//! Settings > Public pages: edit, preview, import, export, and reset the visitor-facing pages.
//! Every route requires a browser session; API keys are refused by the session middleware.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    Extension, Json, Router,
    extract::{
        DefaultBodyLimit, Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::lowercase_hex,
    confirmation::{self, Challenge, Frame, Home, Notice, Rendered, Strings},
    public_pages::{
        self, Bundle, Checked, Compiled, Draft, FORMAT, Invalid, Lang, MAX_LOGO_BYTES, Page,
        VERSION, sanitize::MAX_SOURCE_BYTES,
    },
};

use super::{
    AppState, Problem, RowProblem,
    admin_security::{AuditAction, AuditActor, AuditOutcome, record_audit},
    json_payload,
};

/// Eight pages with the largest HTML and logo, JSON-escaped, fit with room to spare.
const BODY_LIMIT: usize = 2 * 1024 * 1024;
const PREVIEW_SLOTS: usize = 16;
const PREVIEW_LIFETIME: Duration = Duration::from_secs(10 * 60);

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/settings/public-pages",
            get(read_pages).patch(save_page),
        )
        .route("/api/v1/settings/public-pages/reset", post(reset_pages))
        .route(
            "/api/v1/settings/public-pages/preview",
            post(create_preview),
        )
        .route(
            "/api/v1/settings/public-pages/preview/{id}",
            get(show_preview),
        )
        .route(
            "/api/v1/settings/public-pages/import/validate",
            post(validate_import),
        )
        .route("/api/v1/settings/public-pages/import", post(apply_import))
        .route("/api/v1/settings/public-pages/export", get(export_pages))
        .route("/api/v1/settings/public-pages/presets", get(list_presets))
        .layer(Extension(Arc::new(Previews::default())))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

fn invalid(invalid: Invalid) -> Problem {
    Problem::invalid(invalid.code)
}

#[derive(Serialize)]
struct StoredPage {
    #[serde(flatten)]
    draft: Draft,
    updated_at: i64,
}

#[derive(Serialize)]
struct Builtin {
    page: Page,
    lang: Lang,
    title: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    button: Option<String>,
}

#[derive(Serialize)]
struct Limits {
    html_bytes: usize,
    logo_bytes: usize,
}

#[derive(Serialize)]
struct PagesState {
    /// Configured pages. A page and language without an entry uses the built-in page.
    pages: Vec<StoredPage>,
    builtin: Vec<Builtin>,
    limits: Limits,
}

async fn state_of(state: &AppState) -> Result<PagesState, Problem> {
    let stored = public_pages::list(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    let builtin = Page::ALL
        .into_iter()
        .flat_map(|page| {
            Lang::ALL.into_iter().map(move |lang| {
                let (title, message, button) = Strings::of(lang).defaults(page);
                Builtin {
                    page,
                    lang,
                    title,
                    message,
                    button,
                }
            })
        })
        .collect();
    Ok(PagesState {
        pages: stored
            .into_iter()
            .map(|item| StoredPage {
                draft: item.entry.draft(),
                updated_at: item.updated_at,
            })
            .collect(),
        builtin,
        limits: Limits {
            html_bytes: MAX_SOURCE_BYTES,
            logo_bytes: MAX_LOGO_BYTES,
        },
    })
}

async fn read_pages(State(state): State<AppState>) -> Result<Json<PagesState>, Problem> {
    Ok(Json(state_of(&state).await?))
}

#[derive(Serialize)]
struct Saved {
    page: Draft,
    /// What the sanitizer removed from custom markup, as category names.
    removed: Vec<&'static str>,
}

async fn save_page(
    State(state): State<AppState>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Result<Json<Saved>, Problem> {
    let draft = json_payload(body)?;
    let checked = public_pages::validate(&draft).map_err(invalid)?;
    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::save(&mut transaction, &checked.entry)
        .await
        .map_err(|_| Problem::unavailable())?;
    record_audit(
        &mut transaction,
        AuditAction::PublicPagesUpdate,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::changed();
    Ok(Json(Saved {
        page: checked.entry.draft(),
        removed: checked.removed,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetInput {
    page: Option<Page>,
    lang: Option<Lang>,
    #[serde(default)]
    all: bool,
}

async fn reset_pages(
    State(state): State<AppState>,
    body: Result<Json<ResetInput>, JsonRejection>,
) -> Result<Json<PagesState>, Problem> {
    let input = json_payload(body)?;
    let target = match (input.page, input.lang, input.all) {
        (Some(page), Some(lang), false) => Some((page, lang)),
        (None, None, true) => None,
        _ => return Err(Problem::invalid("invalid_reset")),
    };
    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::reset(&mut transaction, target)
        .await
        .map_err(|_| Problem::unavailable())?;
    record_audit(
        &mut transaction,
        AuditAction::PublicPagesReset,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::changed();
    Ok(Json(state_of(&state).await?))
}

// ---------- Preview ----------

/// Rendered drafts for the Settings preview frame. Bounded in count and age, held in memory only.
#[derive(Default)]
struct Previews(Mutex<VecDeque<(String, Instant, Rendered)>>);

impl Previews {
    fn insert(&self, rendered: Rendered) -> Option<String> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).ok()?;
        let id = lowercase_hex(&bytes);
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|(_, created, _)| created.elapsed() < PREVIEW_LIFETIME);
        while entries.len() >= PREVIEW_SLOTS {
            entries.pop_front();
        }
        entries.push_back((id.clone(), Instant::now(), rendered));
        Some(id)
    }

    fn get(&self, id: &str) -> Option<Rendered> {
        let entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .iter()
            .find(|(key, created, _)| key == id && created.elapsed() < PREVIEW_LIFETIME)
            .map(|(_, _, rendered)| rendered.clone())
    }
}

/// Renders a page and language with the public renderer. The preview uses the first public host
/// and a sample root destination, and its form cannot be submitted: the challenge is a
/// placeholder and the frame's sandbox blocks forms.
fn render_preview(state: &AppState, page: Page, lang: Lang, compiled: &Compiled) -> Rendered {
    let host = state
        .hosts
        .all()
        .first()
        .cloned()
        .unwrap_or_else(|| "localhost".to_owned());
    let copy = Strings::of(lang);
    match page {
        Page::Missing => confirmation::render_notice(
            StatusCode::NOT_FOUND,
            Notice::Missing {
                home: Some(Home {
                    href: format!("https://{host}/"),
                    label: "example.com".to_owned(),
                }),
            },
            copy,
            Some(&host),
            Some(compiled),
            Frame::SameOrigin,
        ),
        Page::UnknownHost => confirmation::render_notice(
            StatusCode::NOT_FOUND,
            Notice::UnknownHost,
            copy,
            Some(&host),
            Some(compiled),
            Frame::SameOrigin,
        ),
        Page::Password | Page::Once => confirmation::challenge(
            &Challenge {
                copy,
                origin: &format!("https://{host}"),
                path_and_query: "/example",
                token: "preview",
                password_required: page == Page::Password,
                wrong_password: false,
            },
            Some(compiled),
            Frame::SameOrigin,
        ),
    }
}

#[derive(Serialize)]
struct PreviewCreated {
    /// The admin path that serves the rendered draft for a sandboxed frame.
    url: String,
    removed: Vec<&'static str>,
    /// The sanitized markup of a custom draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
}

fn compiled(checked: &Checked) -> Result<Compiled, Problem> {
    Compiled::new(&checked.entry).map_err(invalid)
}

async fn create_preview(
    State(state): State<AppState>,
    Extension(previews): Extension<Arc<Previews>>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Result<(StatusCode, Json<PreviewCreated>), Problem> {
    let draft = json_payload(body)?;
    let checked = public_pages::validate(&draft).map_err(invalid)?;
    let rendered = render_preview(&state, draft.page, draft.lang, &compiled(&checked)?);
    let id = previews.insert(rendered).ok_or_else(Problem::unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(PreviewCreated {
            url: format!("/api/v1/settings/public-pages/preview/{id}"),
            html: checked.entry.draft().html,
            removed: checked.removed,
        }),
    ))
}

async fn show_preview(
    Extension(previews): Extension<Arc<Previews>>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, Problem> {
    let Ok(Path(id)) = id else {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    };
    let rendered = previews
        .get(&id)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(rendered.response())
}

// ---------- Import and export ----------

#[derive(Serialize)]
struct ImportedPage {
    page: Page,
    lang: Lang,
    mode: public_pages::Mode,
    removed: Vec<&'static str>,
    /// A preview of this page as it would be saved.
    preview: String,
}

#[derive(Serialize)]
struct ImportReport {
    pages: Vec<ImportedPage>,
    /// The sanitized template that an import would store.
    bundle: Bundle,
}

fn bundle_problem((invalid, row): (Invalid, Option<usize>)) -> RowProblem {
    match row {
        Some(row) => RowProblem::at(Problem::invalid(invalid.code), row, invalid.field),
        None => Problem::invalid(invalid.code).into(),
    }
}

fn bundle_of(checked: &[Checked]) -> Bundle {
    Bundle {
        format: FORMAT.to_owned(),
        version: VERSION,
        pages: checked.iter().map(|item| item.entry.draft()).collect(),
    }
}

async fn validate_import(
    State(state): State<AppState>,
    Extension(previews): Extension<Arc<Previews>>,
    body: Result<Json<Bundle>, JsonRejection>,
) -> Result<Json<ImportReport>, RowProblem> {
    let bundle = json_payload(body).map_err(|_| Problem::invalid("invalid_format"))?;
    let checked = public_pages::validate_bundle(&bundle).map_err(bundle_problem)?;
    let mut pages = Vec::with_capacity(checked.len());
    for (index, item) in checked.iter().enumerate() {
        let compiled = Compiled::new(&item.entry).map_err(|invalid| {
            RowProblem::at(Problem::invalid(invalid.code), index + 1, invalid.field)
        })?;
        let rendered = render_preview(&state, item.entry.page, item.entry.lang, &compiled);
        let id = previews.insert(rendered).ok_or_else(Problem::unavailable)?;
        pages.push(ImportedPage {
            page: item.entry.page,
            lang: item.entry.lang,
            mode: item.entry.draft().mode,
            removed: item.removed.clone(),
            preview: format!("/api/v1/settings/public-pages/preview/{id}"),
        });
    }
    Ok(Json(ImportReport {
        bundle: bundle_of(&checked),
        pages,
    }))
}

/// Replaces every page with the template file. Pages that the file does not contain return to
/// the built-in page.
async fn apply_import(
    State(state): State<AppState>,
    body: Result<Json<Bundle>, JsonRejection>,
) -> Result<Json<PagesState>, RowProblem> {
    let bundle = json_payload(body).map_err(|_| Problem::invalid("invalid_format"))?;
    let checked = public_pages::validate_bundle(&bundle).map_err(bundle_problem)?;
    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::reset(&mut transaction, None)
        .await
        .map_err(|_| Problem::unavailable())?;
    for item in &checked {
        public_pages::save(&mut transaction, &item.entry)
            .await
            .map_err(|_| Problem::unavailable())?;
    }
    record_audit(
        &mut transaction,
        AuditAction::PublicPagesImport,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    public_pages::changed();
    Ok(Json(state_of(&state).await?))
}

async fn export_pages(State(state): State<AppState>) -> Result<Response, Problem> {
    let stored = public_pages::list(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    let bundle = Bundle {
        format: FORMAT.to_owned(),
        version: VERSION,
        pages: stored.iter().map(|item| item.entry.draft()).collect(),
    };
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"shortcircuit-public-pages.json\""),
        )],
        Json(bundle),
    )
        .into_response())
}

#[derive(Serialize)]
struct PresetItem {
    id: &'static str,
    name: &'static str,
    bundle: Bundle,
}

#[derive(Serialize)]
struct PresetList {
    items: Vec<PresetItem>,
}

async fn list_presets() -> Json<PresetList> {
    Json(PresetList {
        items: public_pages::presets()
            .into_iter()
            .map(|preset| PresetItem {
                id: preset.id,
                name: preset.name,
                bundle: preset.bundle,
            })
            .collect(),
    })
}
