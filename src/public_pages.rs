//! Administrator-defined public pages: the missing-link, unknown-host, password, and one-time
//! pages that visitors see on the public listener.
//!
//! A page and language either use the built-in page (no stored row), the built-in layout with
//! structured plain-text copy, or sanitized custom HTML. The public listener renders from an
//! in-memory copy that a process-wide change counter refreshes, so a `404` does not read the
//! database while the pages are unchanged. Rendering never fails: an entry that cannot be
//! compiled falls back to the built-in page.

pub(crate) mod sanitize;

use std::{
    collections::BTreeSet,
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::http::HeaderValue;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};

use crate::confirmation::{STYLE, base64, escape};

pub(crate) use sanitize::{Part, Slot};

pub(crate) const FORMAT: &str = "shortcircuit-public-pages";
pub(crate) const VERSION: u32 = 1;
pub(crate) const MAX_LOGO_BYTES: usize = 64 * 1024;
const MAX_TITLE: usize = 120;
const MAX_MESSAGE: usize = 600;
const MAX_BUTTON: usize = 60;
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Page {
    Missing,
    UnknownHost,
    Password,
    Once,
}

impl Page {
    pub(crate) const ALL: [Self; 4] =
        [Self::Missing, Self::UnknownHost, Self::Password, Self::Once];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::UnknownHost => "unknown_host",
            Self::Password => "password",
            Self::Once => "once",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.name() == value)
    }

    /// Password and one-time pages contain the server's form.
    pub(crate) fn has_form(self) -> bool {
        matches!(self, Self::Password | Self::Once)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Lang {
    En,
    De,
}

impl Lang {
    pub(crate) const ALL: [Self; 2] = [Self::En, Self::De];

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.code() == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Structured,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageKind {
    Png,
    Jpeg,
    Webp,
}

impl ImageKind {
    pub(crate) fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [Self::Png, Self::Jpeg, Self::Webp]
            .into_iter()
            .find(|kind| kind.mime() == value)
    }

    /// The format that the file signature identifies.
    pub(crate) fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Self::Jpeg)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }
}

/// Decodes standard base64 with optional padding. Returns `None` for any other character.
pub(crate) fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let input = input.trim_end_matches('=');
    if input.len() % 4 == 1 {
        return None;
    }
    let mut decoded = Vec::with_capacity(input.len() / 4 * 3 + 2);
    let mut buffer = 0_u32;
    let mut bits = 0;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(decoded)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogoData {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    /// Standard base64 of the image file.
    pub(crate) data: String,
}

/// One page and language as the API and the template file carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    pub(crate) page: Page,
    pub(crate) lang: Lang,
    pub(crate) mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) button: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) logo: Option<LogoData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) html: Option<String>,
}

/// The template file: every configured page, mode, and language.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bundle {
    pub(crate) format: String,
    pub(crate) version: u32,
    pub(crate) pages: Vec<Draft>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Logo {
    pub(crate) kind: ImageKind,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    Structured {
        title: Option<String>,
        message: Option<String>,
        button: Option<String>,
        accent: Option<String>,
        logo: Option<Logo>,
    },
    /// Sanitized markup.
    Custom { html: String },
}

/// A validated page and language, ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) page: Page,
    pub(crate) lang: Lang,
    pub(crate) content: Content,
}

/// A validation failure with the field it concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Invalid {
    pub(crate) code: &'static str,
    pub(crate) field: Option<&'static str>,
}

impl Invalid {
    fn at(code: &'static str, field: &'static str) -> Self {
        Self {
            code,
            field: Some(field),
        }
    }
}

/// The result of validating a draft: the entry and what the sanitizer removed.
pub(crate) struct Checked {
    pub(crate) entry: Entry,
    pub(crate) removed: Vec<&'static str>,
}

fn plain_text(
    value: Option<&str>,
    limit: usize,
    field: &'static str,
    lines: bool,
) -> Result<Option<String>, Invalid> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > limit {
        return Err(Invalid::at("text_too_long", field));
    }
    if value
        .chars()
        .any(|c| c.is_control() && !(lines && c == '\n'))
    {
        return Err(Invalid::at("invalid_text", field));
    }
    Ok(Some(value.to_owned()))
}

/// The WCAG relative luminance of a `#rrggbb` color.
fn luminance(hex: &str) -> f64 {
    let channel = |index: usize| {
        let value = f64::from(u8::from_str_radix(&hex[index..index + 2], 16).unwrap_or(0)) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

fn contrast(first: f64, second: f64) -> f64 {
    let (light, dark) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (light + 0.05) / (dark + 0.05)
}

const ON_DARK: &str = "#101115";
const ON_LIGHT: &str = "#ffffff";

/// Button text for an accent: the Polar Signal dark text or white, whichever contrasts more.
/// Returns `None` when neither reaches the 4.5:1 that the palette requires for text.
fn on_accent(accent: &str) -> Option<&'static str> {
    let accent = luminance(accent);
    let dark = contrast(accent, luminance(ON_DARK));
    let light = contrast(accent, luminance(ON_LIGHT));
    let (color, ratio) = if dark >= light {
        (ON_DARK, dark)
    } else {
        (ON_LIGHT, light)
    };
    (ratio >= 4.5).then_some(color)
}

fn accent(value: Option<&str>) -> Result<Option<String>, Invalid> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let value = value.to_ascii_lowercase();
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Invalid::at("invalid_accent", "accent"));
    }
    if on_accent(&value).is_none() {
        return Err(Invalid::at("accent_contrast", "accent"));
    }
    Ok(Some(value))
}

fn logo(value: Option<&LogoData>) -> Result<Option<Logo>, Invalid> {
    let Some(value) = value else {
        return Ok(None);
    };
    let kind = ImageKind::parse(&value.kind).ok_or(Invalid::at("invalid_logo", "logo"))?;
    // Base64 of the largest logo, so an oversized value is refused before decoding.
    if value.data.len() > MAX_LOGO_BYTES.div_ceil(3) * 4 {
        return Err(Invalid::at("logo_too_large", "logo"));
    }
    let bytes = decode_base64(&value.data).ok_or(Invalid::at("invalid_logo", "logo"))?;
    if bytes.is_empty() || ImageKind::sniff(&bytes) != Some(kind) {
        return Err(Invalid::at("invalid_logo", "logo"));
    }
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(Invalid::at("logo_too_large", "logo"));
    }
    Ok(Some(Logo { kind, bytes }))
}

/// Validates a draft. Custom HTML is sanitized; the result holds the sanitized markup.
pub(crate) fn validate(draft: &Draft) -> Result<Checked, Invalid> {
    let (content, removed) = match draft.mode {
        Mode::Structured => {
            if draft.html.is_some() {
                return Err(Invalid::at("invalid_mode_fields", "html"));
            }
            let button = plain_text(draft.button.as_deref(), MAX_BUTTON, "button", false)?;
            if button.is_some() && draft.page == Page::UnknownHost {
                return Err(Invalid::at("button_not_supported", "button"));
            }
            (
                Content::Structured {
                    title: plain_text(draft.title.as_deref(), MAX_TITLE, "title", false)?,
                    message: plain_text(draft.message.as_deref(), MAX_MESSAGE, "message", true)?,
                    button,
                    accent: accent(draft.accent.as_deref())?,
                    logo: logo(draft.logo.as_ref())?,
                },
                Vec::new(),
            )
        }
        Mode::Custom => {
            if draft.title.is_some()
                || draft.message.is_some()
                || draft.button.is_some()
                || draft.accent.is_some()
                || draft.logo.is_some()
            {
                return Err(Invalid::at("invalid_mode_fields", "mode"));
            }
            let html = draft
                .html
                .as_deref()
                .filter(|html| !html.trim().is_empty())
                .ok_or(Invalid::at("html_required", "html"))?;
            let sanitized = custom(draft.page, html)?;
            (
                Content::Custom {
                    html: sanitized.source,
                },
                sanitized.removed.into_iter().collect(),
            )
        }
    };
    Ok(Checked {
        entry: Entry {
            page: draft.page,
            lang: draft.lang,
            content,
        },
        removed,
    })
}

fn custom(page: Page, html: &str) -> Result<sanitize::Sanitized, Invalid> {
    let sanitized = sanitize::sanitize(html).map_err(|code| Invalid::at(code, "html"))?;
    if sanitized.body.iter().all(|part| match part {
        Part::Markup(text) => text.trim().is_empty(),
        Part::Slot(_) => false,
    }) && sanitized.title.is_none()
    {
        return Err(Invalid::at("html_required", "html"));
    }
    match (page.has_form(), sanitized.forms) {
        (true, 1) | (false, 0) => Ok(sanitized),
        (true, 0) => Err(Invalid::at("form_required", "html")),
        (true, _) => Err(Invalid::at("form_duplicate", "html")),
        (false, _) => Err(Invalid::at("form_not_supported", "html")),
    }
}

impl Entry {
    pub(crate) fn draft(&self) -> Draft {
        let mut draft = Draft {
            page: self.page,
            lang: self.lang,
            mode: Mode::Structured,
            title: None,
            message: None,
            button: None,
            accent: None,
            logo: None,
            html: None,
        };
        match &self.content {
            Content::Structured {
                title,
                message,
                button,
                accent,
                logo,
            } => {
                draft.title.clone_from(title);
                draft.message.clone_from(message);
                draft.button.clone_from(button);
                draft.accent.clone_from(accent);
                draft.logo = logo.as_ref().map(|logo| LogoData {
                    kind: logo.kind.mime().to_owned(),
                    data: base64(&logo.bytes),
                });
            }
            Content::Custom { html } => {
                draft.mode = Mode::Custom;
                draft.html = Some(html.clone());
            }
        }
        draft
    }
}

/// Checks a template file. Every entry passes the same validation as a single edit.
pub(crate) fn validate_bundle(bundle: &Bundle) -> Result<Vec<Checked>, (Invalid, Option<usize>)> {
    if bundle.format != FORMAT {
        return Err((
            Invalid {
                code: "invalid_format",
                field: None,
            },
            None,
        ));
    }
    if bundle.version != VERSION {
        return Err((
            Invalid {
                code: "unsupported_version",
                field: None,
            },
            None,
        ));
    }
    if bundle.pages.len() > Page::ALL.len() * Lang::ALL.len() {
        return Err((
            Invalid {
                code: "too_many_pages",
                field: None,
            },
            None,
        ));
    }
    let mut seen = BTreeSet::new();
    let mut checked = Vec::with_capacity(bundle.pages.len());
    for (index, draft) in bundle.pages.iter().enumerate() {
        if !seen.insert((draft.page, draft.lang)) {
            return Err((Invalid::at("duplicate_page", "page"), Some(index + 1)));
        }
        checked.push(validate(draft).map_err(|invalid| (invalid, Some(index + 1)))?);
    }
    Ok(checked)
}

// ---------- Compiled pages ----------

/// A page and language prepared for rendering.
#[derive(Debug)]
pub(crate) struct Compiled {
    pub(crate) look: Look,
    /// `style-src` sources: the hash of every style element the page contains.
    style_sources: String,
}

#[derive(Debug)]
pub(crate) enum Look {
    Structured {
        /// Escaped copy. `None` keeps the built-in text.
        title: Option<String>,
        message: Option<String>,
        button: Option<String>,
        /// A second style element for the accent and logo, after the built-in one.
        style: Option<String>,
        /// The logo as a `data:` URI. It replaces the mark.
        logo: Option<String>,
    },
    Custom {
        title: Option<Vec<Part>>,
        styles: Vec<String>,
        body: Vec<Part>,
    },
}

fn hash_source(style: &str) -> String {
    format!("'sha256-{}'", base64(&Sha256::digest(style.as_bytes())))
}

impl Compiled {
    /// Prepares an entry. Custom markup is sanitized again, so stored markup is never served
    /// without passing the current sanitizer.
    pub(crate) fn new(entry: &Entry) -> Result<Self, Invalid> {
        match &entry.content {
            Content::Structured {
                title,
                message,
                button,
                accent,
                logo,
            } => {
                let mut style = String::new();
                if let Some(accent) = accent {
                    let on = on_accent(accent).ok_or(Invalid::at("accent_contrast", "accent"))?;
                    style.push_str(&format!(
                        ":root{{--accent:{accent};--accent-hover:{accent};--accent-active:{accent};\
                         --accent-soft:{accent}40;--on-accent:{on}}}\
                         .primary:hover,.primary:active{{filter:brightness(1.08)}}"
                    ));
                }
                if logo.is_some() {
                    style.push_str(
                        ".logo{display:block;width:auto;height:auto;max-width:160px;max-height:48px}",
                    );
                }
                let mut style_sources = hash_source(STYLE);
                let style = (!style.is_empty()).then(|| {
                    style_sources.push(' ');
                    style_sources.push_str(&hash_source(&style));
                    style
                });
                Ok(Self {
                    look: Look::Structured {
                        title: title.as_deref().map(escape),
                        message: message
                            .as_deref()
                            .map(|text| escape(text).replace('\n', "<br>")),
                        button: button.as_deref().map(escape),
                        style,
                        logo: logo.as_ref().map(|logo| {
                            format!("data:{};base64,{}", logo.kind.mime(), base64(&logo.bytes))
                        }),
                    },
                    style_sources,
                })
            }
            Content::Custom { html } => {
                let sanitized = custom(entry.page, html)?;
                let style_sources = sanitized
                    .styles
                    .iter()
                    .map(|style| hash_source(style))
                    .collect::<Vec<_>>()
                    .join(" ");
                Ok(Self {
                    look: Look::Custom {
                        title: sanitized.title,
                        styles: sanitized.styles,
                        body: sanitized.body,
                    },
                    style_sources,
                })
            }
        }
    }

    /// The Content Security Policy: no scripts, inline images only, styles by hash, forms only
    /// to the page's own origin, and framing as requested.
    pub(crate) fn policy(&self, frame_ancestors: &str) -> HeaderValue {
        let styles = if self.style_sources.is_empty() {
            String::new()
        } else {
            format!(" style-src {};", self.style_sources)
        };
        HeaderValue::from_str(&format!(
            "default-src 'none';{styles} img-src data:; form-action 'self'; base-uri 'none'; \
             frame-ancestors {frame_ancestors}"
        ))
        .expect("the policy is visible ASCII")
    }
}

/// Every compiled page and language. A missing entry uses the built-in page.
#[derive(Debug, Default)]
pub(crate) struct Templates(Vec<((Page, Lang), Compiled)>);

impl Templates {
    pub(crate) fn get(&self, page: Page, lang: Lang) -> Option<&Compiled> {
        self.0
            .iter()
            .find(|(key, _)| *key == (page, lang))
            .map(|(_, compiled)| compiled)
    }
}

// ---------- Storage ----------

type Row = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
    i64,
);

/// A stored entry with its modification time.
pub(crate) struct Stored {
    pub(crate) entry: Entry,
    pub(crate) updated_at: i64,
}

fn from_row(row: Row) -> Option<Stored> {
    let (page, lang, mode, title, message, button, accent, logo_type, logo, html, updated_at) = row;
    let page = Page::parse(&page)?;
    let lang = Lang::parse(&lang)?;
    let content = match mode.as_str() {
        "structured" => Content::Structured {
            title,
            message,
            button,
            accent,
            logo: match (logo_type, logo) {
                (Some(kind), Some(bytes)) => Some(Logo {
                    kind: ImageKind::parse(&kind)?,
                    bytes,
                }),
                _ => None,
            },
        },
        "custom" => Content::Custom { html: html? },
        _ => return None,
    };
    Some(Stored {
        entry: Entry {
            page,
            lang,
            content,
        },
        updated_at,
    })
}

pub(crate) async fn list(pool: &SqlitePool) -> Result<Vec<Stored>, sqlx::Error> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT page, lang, mode, title, message, button, accent, logo_type, logo, html, updated_at
         FROM public_pages ORDER BY page, lang",
    )
    .fetch_all(pool)
    .await?;
    let mut stored: Vec<_> = rows.into_iter().filter_map(from_row).collect();
    stored.sort_by_key(|item| (item.entry.page, item.entry.lang));
    Ok(stored)
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    entry: &Entry,
) -> Result<(), sqlx::Error> {
    let (mode, title, message, button, accent, logo, html) = match &entry.content {
        Content::Structured {
            title,
            message,
            button,
            accent,
            logo,
        } => (
            "structured",
            title.as_deref(),
            message.as_deref(),
            button.as_deref(),
            accent.as_deref(),
            logo.as_ref(),
            None,
        ),
        Content::Custom { html } => ("custom", None, None, None, None, None, Some(html.as_str())),
    };
    sqlx::query(
        "INSERT INTO public_pages
            (page, lang, mode, title, message, button, accent, logo_type, logo, html, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())
         ON CONFLICT (page, lang) DO UPDATE SET
            mode = excluded.mode, title = excluded.title, message = excluded.message,
            button = excluded.button, accent = excluded.accent, logo_type = excluded.logo_type,
            logo = excluded.logo, html = excluded.html, updated_at = excluded.updated_at",
    )
    .bind(entry.page.name())
    .bind(entry.lang.code())
    .bind(mode)
    .bind(title)
    .bind(message)
    .bind(button)
    .bind(accent)
    .bind(logo.map(|logo| logo.kind.mime()))
    .bind(logo.map(|logo| logo.bytes.as_slice()))
    .bind(html)
    .execute(connection)
    .await?;
    Ok(())
}

/// Removes one page and language, or every entry when `target` is `None`.
pub(crate) async fn reset(
    connection: &mut SqliteConnection,
    target: Option<(Page, Lang)>,
) -> Result<u64, sqlx::Error> {
    let result = match target {
        Some((page, lang)) => {
            sqlx::query("DELETE FROM public_pages WHERE page = ? AND lang = ?")
                .bind(page.name())
                .bind(lang.code())
                .execute(connection)
                .await?
        }
        None => {
            sqlx::query("DELETE FROM public_pages")
                .execute(connection)
                .await?
        }
    };
    Ok(result.rows_affected())
}

// ---------- In-memory copy ----------

/// Incremented after every committed change. Each public listener compares it with the value
/// it loaded and reads the table again only when it differs.
static GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn changed() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

struct Loaded {
    generation: u64,
    retry_at: Option<Instant>,
    templates: Arc<Templates>,
}

/// The public listener's copy of the compiled pages.
#[derive(Clone)]
pub(crate) struct Cache {
    loaded: Arc<RwLock<Loaded>>,
    loading: Arc<tokio::sync::Mutex<()>>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            loaded: Arc::new(RwLock::new(Loaded {
                generation: 0,
                retry_at: None,
                templates: Arc::default(),
            })),
            loading: Arc::default(),
        }
    }
}

impl Cache {
    fn fresh(&self, generation: u64) -> Option<Arc<Templates>> {
        let loaded = self.loaded.read().unwrap_or_else(PoisonError::into_inner);
        let waiting = loaded
            .retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at);
        (loaded.generation == generation || waiting).then(|| Arc::clone(&loaded.templates))
    }

    /// The current pages. A read error keeps the previous copy, which is built-in before the
    /// first successful read, and retries after a short delay.
    pub(crate) async fn current(&self, pool: &SqlitePool) -> Arc<Templates> {
        let generation = GENERATION.load(Ordering::Acquire);
        if let Some(templates) = self.fresh(generation) {
            return templates;
        }
        let _loading = self.loading.lock().await;
        if let Some(templates) = self.fresh(generation) {
            return templates;
        }
        let result = list(pool).await;
        let mut loaded = self.loaded.write().unwrap_or_else(PoisonError::into_inner);
        match result {
            Ok(stored) => {
                let mut compiled = Vec::with_capacity(stored.len());
                for item in stored {
                    match Compiled::new(&item.entry) {
                        Ok(page) => compiled.push(((item.entry.page, item.entry.lang), page)),
                        Err(invalid) => tracing::warn!(
                            page = item.entry.page.name(),
                            lang = item.entry.lang.code(),
                            code = invalid.code,
                            "a stored public page is invalid; the built-in page is used"
                        ),
                    }
                }
                loaded.generation = generation;
                loaded.retry_at = None;
                loaded.templates = Arc::new(Templates(compiled));
            }
            Err(_) => {
                tracing::warn!("public pages could not be read; the previous pages are used");
                loaded.retry_at = Some(Instant::now() + RETRY_AFTER_ERROR);
            }
        }
        Arc::clone(&loaded.templates)
    }
}

// ---------- Starter templates ----------

pub(crate) struct Preset {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) bundle: Bundle,
}

fn structured(page: Page, lang: Lang, title: &str, message: &str, button: Option<&str>) -> Draft {
    Draft {
        page,
        lang,
        mode: Mode::Structured,
        title: Some(title.to_owned()),
        message: Some(message.to_owned()),
        button: button.map(str::to_owned),
        accent: Some("#ff79c6".to_owned()),
        logo: None,
        html: None,
    }
}

const MINIMAL_STYLE: &str = "<style>:root{color-scheme:light dark}\
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px;\
font:16px/1.5 ui-sans-serif,-apple-system,\"Segoe UI\",Roboto,Arial,sans-serif;background:#fafafa;color:#1d1d1f}\
main{max-width:420px}h1{font-size:22px;line-height:1.3;margin:0 0 8px}p{margin:0 0 16px;color:#555}\
a{color:inherit}form{display:grid;gap:12px}label{font-weight:600}\
input{font:inherit;padding:8px 10px;border:1px solid #888;border-radius:6px}\
.btn{font:inherit;padding:8px 14px;border:1px solid #1d1d1f;border-radius:6px;background:#1d1d1f;color:#fff;\
text-decoration:none;display:inline-block;cursor:pointer}.error{color:#b3261e}\
@media (prefers-color-scheme:dark){body{background:#121212;color:#f2f2f2}p{color:#b8b8b8}\
input{background:#1e1e1e;color:#f2f2f2}.btn{background:#f2f2f2;color:#121212;border-color:#f2f2f2}.error{color:#ff8a80}}</style>";

fn minimal(page: Page, lang: Lang, title: &str, text: &str) -> Draft {
    let extra = match page {
        Page::Missing => "{{root_link}}",
        Page::UnknownHost => "",
        Page::Password | Page::Once => "{{form}}",
    };
    Draft {
        page,
        lang,
        mode: Mode::Custom,
        title: None,
        message: None,
        button: None,
        accent: None,
        logo: None,
        html: Some(format!(
            "<title>{title}</title>{MINIMAL_STYLE}<main><h1>{title}</h1><p>{text}</p>{extra}</main>"
        )),
    }
}

pub(crate) fn presets() -> Vec<Preset> {
    use Lang::{De, En};
    use Page::{Missing, Once, Password, UnknownHost};
    let bundle = |pages| Bundle {
        format: FORMAT.to_owned(),
        version: VERSION,
        pages,
    };
    vec![
        Preset {
            id: "polar-signal",
            name: "Polar Signal",
            bundle: bundle(vec![
                structured(
                    Missing,
                    En,
                    "This link isn’t available",
                    "The short link may be mistyped, paused, or used up.",
                    None,
                ),
                structured(
                    Missing,
                    De,
                    "Dieser Link ist nicht verfügbar",
                    "Der Kurzlink ist vielleicht falsch geschrieben, pausiert oder aufgebraucht.",
                    None,
                ),
                structured(
                    UnknownHost,
                    En,
                    "Nothing is served here",
                    "This address doesn’t serve short links.",
                    None,
                ),
                structured(
                    UnknownHost,
                    De,
                    "Hier wird nichts bereitgestellt",
                    "Diese Adresse stellt keine Kurzlinks bereit.",
                    None,
                ),
                structured(
                    Password,
                    En,
                    "Enter the password",
                    "This link is protected. Enter the password you received with it.",
                    Some("Open link"),
                ),
                structured(
                    Password,
                    De,
                    "Passwort eingeben",
                    "Dieser Link ist geschützt. Gib das Passwort ein, das du dazu erhalten hast.",
                    Some("Link öffnen"),
                ),
                structured(
                    Once,
                    En,
                    "Open this link once",
                    "This link works for one visit. After you continue, it can’t be used again.",
                    Some("Open link"),
                ),
                structured(
                    Once,
                    De,
                    "Diesen Link einmal öffnen",
                    "Dieser Link funktioniert für einen Besuch. Danach kann ihn niemand mehr verwenden.",
                    Some("Link öffnen"),
                ),
            ]),
        },
        Preset {
            id: "minimal",
            name: "Minimal",
            bundle: bundle(vec![
                minimal(
                    Missing,
                    En,
                    "Link not found",
                    "Check the address. Short links are case-sensitive.",
                ),
                minimal(
                    Missing,
                    De,
                    "Link nicht gefunden",
                    "Prüfe die Adresse. Kurzlinks unterscheiden Groß- und Kleinschreibung.",
                ),
                minimal(
                    UnknownHost,
                    En,
                    "Nothing here",
                    "{{host}} doesn’t serve short links.",
                ),
                minimal(
                    UnknownHost,
                    De,
                    "Hier ist nichts",
                    "{{host}} stellt keine Kurzlinks bereit.",
                ),
                minimal(
                    Password,
                    En,
                    "Password required",
                    "Enter the password to open this link.",
                ),
                minimal(
                    Password,
                    De,
                    "Passwort erforderlich",
                    "Gib das Passwort ein, um diesen Link zu öffnen.",
                ),
                minimal(
                    Once,
                    En,
                    "One visit only",
                    "This link opens once. After that it stops working.",
                ),
                minimal(
                    Once,
                    De,
                    "Nur ein Besuch",
                    "Dieser Link öffnet sich einmal. Danach funktioniert er nicht mehr.",
                ),
            ]),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn draft(page: Page, mode: Mode) -> Draft {
        Draft {
            page,
            lang: Lang::En,
            mode,
            title: None,
            message: None,
            button: None,
            accent: None,
            logo: None,
            html: None,
        }
    }

    #[test]
    fn base64_round_trips_and_refuses_other_characters() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"\x00\xff\x10"] {
            assert_eq!(decode_base64(&base64(sample)).as_deref(), Some(sample));
        }
        assert_eq!(decode_base64("Zm9v YQ=="), None);
        assert_eq!(decode_base64("Z"), None);
        assert_eq!(decode_base64("Zm9-"), None);
    }

    #[test]
    fn the_form_placeholder_is_required_exactly_where_a_form_belongs() {
        for page in [Page::Password, Page::Once] {
            let mut input = draft(page, Mode::Custom);
            input.html = Some("<h1>Protected</h1>".into());
            assert_eq!(validate(&input).err().unwrap().code, "form_required");
            input.html = Some("<h1>Protected</h1>{{form}}{{form}}".into());
            assert_eq!(validate(&input).err().unwrap().code, "form_duplicate");
            input.html = Some("<h1>Protected</h1><div>{{form}}</div>".into());
            assert!(validate(&input).is_ok());
            // A form placeholder inside a removed element does not count.
            input.html = Some("<h1>P</h1><script>{{form}}</script>".into());
            assert_eq!(validate(&input).err().unwrap().code, "form_required");
        }
        for page in [Page::Missing, Page::UnknownHost] {
            let mut input = draft(page, Mode::Custom);
            input.html = Some("<h1>Gone</h1>{{form}}".into());
            assert_eq!(validate(&input).err().unwrap().code, "form_not_supported");
        }
    }

    #[test]
    fn structured_fields_are_validated() {
        let mut input = draft(Page::Missing, Mode::Structured);
        input.title = Some("  Gone  ".into());
        input.accent = Some("#FF79C6".into());
        input.logo = Some(LogoData {
            kind: "image/png".into(),
            data: PNG.into(),
        });
        let checked = validate(&input).unwrap();
        let Content::Structured {
            title,
            accent,
            logo,
            ..
        } = &checked.entry.content
        else {
            panic!("structured");
        };
        assert_eq!(title.as_deref(), Some("Gone"));
        assert_eq!(accent.as_deref(), Some("#ff79c6"));
        assert_eq!(logo.as_ref().unwrap().kind, ImageKind::Png);
        assert_eq!(checked.entry.draft().logo.unwrap().data, PNG);

        for (field, value, code) in [
            ("accent", "red", "invalid_accent"),
            ("accent", "#12345", "invalid_accent"),
            ("accent", "#ff79c6;}", "invalid_accent"),
            ("title", "a\u{7}b", "invalid_text"),
            ("title", &"x".repeat(121), "text_too_long"),
        ] {
            let mut input = draft(Page::Missing, Mode::Structured);
            match field {
                "accent" => input.accent = Some(value.to_owned()),
                _ => input.title = Some(value.to_owned()),
            }
            assert_eq!(
                validate(&input).err().unwrap().code,
                code,
                "{field}={value}"
            );
        }
        // Neither dark nor white text reaches 4.5:1 on this mid tone.
        assert!(on_accent("#e0457b").is_some() || on_accent("#d0408e").is_some());
        let mid = (0..=255_u8)
            .map(|v| format!("#{v:02x}{v:02x}{v:02x}"))
            .find(|color| on_accent(color).is_none())
            .expect("a mid gray fails both");
        let mut input = draft(Page::Missing, Mode::Structured);
        input.accent = Some(mid);
        assert_eq!(validate(&input).err().unwrap().code, "accent_contrast");

        let mut svg = draft(Page::Missing, Mode::Structured);
        svg.logo = Some(LogoData {
            kind: "image/svg+xml".into(),
            data: base64(b"<svg/>"),
        });
        assert_eq!(validate(&svg).err().unwrap().code, "invalid_logo");
        svg.logo = Some(LogoData {
            kind: "image/png".into(),
            data: base64(b"<svg/>"),
        });
        assert_eq!(validate(&svg).err().unwrap().code, "invalid_logo");
        let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
        big.resize(MAX_LOGO_BYTES + 1, 0);
        svg.logo = Some(LogoData {
            kind: "image/png".into(),
            data: base64(&big),
        });
        assert_eq!(validate(&svg).err().unwrap().code, "logo_too_large");

        let mut unknown = draft(Page::UnknownHost, Mode::Structured);
        unknown.button = Some("Go".into());
        assert_eq!(
            validate(&unknown).err().unwrap().code,
            "button_not_supported"
        );
        let mut mixed = draft(Page::Missing, Mode::Custom);
        mixed.html = Some("<p>x</p>".into());
        mixed.title = Some("t".into());
        assert_eq!(validate(&mixed).err().unwrap().code, "invalid_mode_fields");
    }

    #[test]
    fn bundles_are_checked_for_format_version_size_and_duplicates() {
        let page = draft(Page::Missing, Mode::Structured);
        let bundle = |format: &str, version, pages: Vec<Draft>| Bundle {
            format: format.into(),
            version,
            pages,
        };
        assert_eq!(
            validate_bundle(&bundle("other", 1, vec![]))
                .err()
                .unwrap()
                .0
                .code,
            "invalid_format"
        );
        assert_eq!(
            validate_bundle(&bundle(FORMAT, 2, vec![]))
                .err()
                .unwrap()
                .0
                .code,
            "unsupported_version"
        );
        assert_eq!(
            validate_bundle(&bundle(FORMAT, 1, vec![page.clone(); 9]))
                .err()
                .unwrap()
                .0
                .code,
            "too_many_pages"
        );
        let duplicate = validate_bundle(&bundle(FORMAT, 1, vec![page.clone(), page.clone()]));
        assert_eq!(
            duplicate.err().unwrap(),
            (Invalid::at("duplicate_page", "page"), Some(2))
        );
        let mut large = draft(Page::UnknownHost, Mode::Custom);
        large.html = Some(format!("<p>{}</p>", "a".repeat(sanitize::MAX_SOURCE_BYTES)));
        assert_eq!(
            validate_bundle(&bundle(FORMAT, 1, vec![page, large]))
                .err()
                .unwrap(),
            (Invalid::at("page_too_large", "html"), Some(2))
        );
    }

    #[test]
    fn every_preset_is_a_valid_complete_bundle() {
        for preset in presets() {
            let checked = validate_bundle(&preset.bundle)
                .unwrap_or_else(|error| panic!("{}: {error:?}", preset.id));
            assert_eq!(checked.len(), 8, "{}", preset.id);
            for item in &checked {
                assert!(item.removed.is_empty(), "{}: {:?}", preset.id, item.removed);
                Compiled::new(&item.entry).unwrap();
            }
            let json = serde_json::to_string(&preset.bundle).unwrap();
            let parsed: Bundle = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.pages, preset.bundle.pages);
        }
    }

    #[test]
    fn policies_list_only_hashed_styles_and_inline_images() {
        let mut input = draft(Page::Missing, Mode::Custom);
        input.html = Some("<style>p{color:red}</style><p>x</p>".into());
        let compiled = Compiled::new(&validate(&input).unwrap().entry).unwrap();
        let policy = compiled.policy("'none'");
        let policy = policy.to_str().unwrap();
        assert!(policy.starts_with("default-src 'none'; style-src 'sha256-"));
        assert!(policy.contains(
            "img-src data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
        ));
        assert!(!policy.contains("unsafe") && !policy.contains("script-src"));
        assert_eq!(policy.matches("'sha256-").count(), 1);
    }
}
