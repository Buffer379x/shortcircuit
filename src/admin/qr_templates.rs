use axum::{
    Json, Router,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};

use crate::qr::{Options, QrError};

use super::{AppState, Problem, json_payload, resource_id};

const MAX_TEMPLATES: i64 = 32;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/qr/templates", get(list).post(create))
        .route("/api/v1/qr/templates/{id}", delete(remove))
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
struct Settings {
    name: String,
    size: u16,
    foreground: String,
    background: String,
    transparent: bool,
    correction: String,
    quiet_zone: i64,
    #[serde(default = "default_frame")]
    frame: String,
}

fn default_frame() -> String {
    "none".to_owned()
}

#[derive(Serialize, sqlx::FromRow)]
struct Template {
    id: i64,
    #[serde(flatten)]
    #[sqlx(flatten)]
    settings: Settings,
}

#[derive(Serialize)]
struct TemplateList {
    items: Vec<Template>,
}

async fn list(State(state): State<AppState>) -> Result<Json<TemplateList>, Problem> {
    let items = sqlx::query_as(
        "SELECT id, name, size, foreground, background, transparent, correction, quiet_zone, frame
         FROM qr_templates ORDER BY id DESC LIMIT ?",
    )
    .bind(MAX_TEMPLATES)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| Problem::unavailable())?;
    Ok(Json(TemplateList { items }))
}

async fn create(
    State(state): State<AppState>,
    body: Result<Json<Settings>, JsonRejection>,
) -> Result<(StatusCode, Json<Template>), Problem> {
    let input = json_payload(body)?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(Problem::invalid("invalid_qr_template_name"));
    }
    let options = Options {
        size: Some(input.size),
        foreground: Some(input.foreground.to_ascii_lowercase()),
        background: Some(input.background.to_ascii_lowercase()),
        transparent: Some(input.transparent),
        correction: Some(input.correction.clone()),
        quiet_zone: Some(input.quiet_zone),
        frame: Some(input.frame.clone()),
    };
    options.validate()?;
    // SQLite serializes writes. Checking capacity in the insert prevents concurrent overflow.
    let template = sqlx::query_as(
        "INSERT INTO qr_templates (name, size, foreground, background, transparent, correction, quiet_zone, frame)
         SELECT ?, ?, ?, ?, ?, ?, ?, ? WHERE (SELECT count(*) FROM qr_templates) < ?
         RETURNING id, name, size, foreground, background, transparent, correction, quiet_zone, frame",
    )
    .bind(name)
    .bind(input.size)
    .bind(options.foreground)
    .bind(options.background)
    .bind(input.transparent)
    .bind(input.correction)
    .bind(input.quiet_zone)
    .bind(input.frame)
    .bind(MAX_TEMPLATES)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|failure| failure.is_unique_violation())
        {
            Problem(StatusCode::CONFLICT, "qr_template_exists")
        } else {
            Problem::unavailable()
        }
    })?
    .ok_or(Problem(StatusCode::CONFLICT, "qr_template_limit"))?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn remove(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    let removed = sqlx::query("DELETE FROM qr_templates WHERE id = ?")
        .bind(resource_id(id)?)
        .execute(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    if removed.rows_affected() == 0 {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

impl From<QrError> for Problem {
    fn from(error: QrError) -> Self {
        match error {
            QrError::InvalidSize => Self::invalid("invalid_qr_size"),
            QrError::InvalidColor => Self::invalid("invalid_qr_color"),
            QrError::InvalidContrast => Self::invalid("invalid_qr_contrast"),
            QrError::InvalidCorrection => Self::invalid("invalid_qr_correction"),
            QrError::InvalidQuietZone => Self::invalid("invalid_qr_quiet_zone"),
            QrError::InvalidFrame => Self::invalid("invalid_qr_frame"),
            QrError::InvalidLogo => Self::invalid("invalid_qr_logo"),
            QrError::InvalidLogoDimensions => Self::invalid("invalid_qr_logo_dimensions"),
            QrError::InvalidLogoCorrection => Self::invalid("invalid_qr_logo_correction"),
            QrError::LogoUnreadable => Self::invalid("qr_logo_unreadable"),
            QrError::DataTooLong => Self::invalid("qr_data_too_long"),
            QrError::EncodingFailed => Self::unavailable(),
        }
    }
}
