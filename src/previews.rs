//! Opt-in preview images for saved destinations.
//!
//! A preview is the image that the destination page itself offers, as messengers show it:
//! `og:image`, then `twitter:image`, otherwise the page icon (`<link rel="icon">` or
//! `/favicon.ico`). No page is rendered and no script runs. The image URL comes from the same
//! page response that title lookup reads, so a preview never costs a second page request.
//!
//! Every image fetch goes through the public outbound policy in `outbound`, with its own size
//! cap and deadline, and through the destination policy. The bytes are decoded only by the
//! pure-Rust PNG, JPEG, WebP, and GIF decoders of the `image` crate, under dimension and
//! allocation limits, on a blocking thread. SVG and every other format are refused. The result
//! is resized to a small thumbnail and re-encoded as a baseline JPEG; only those bytes are
//! stored, so no original file, metadata, EXIF, or color profile is kept. Only the origin of the
//! image URL is recorded, because the full URL can carry tokens.
//!
//! Previews run in the title lookup worker and need both switches: title lookup and preview
//! images. Links with password or one-time protection are never fetched.

use std::io::Cursor;

use image::{
    DynamicImage, ExtendedColorType, ImageError, ImageFormat, ImageReader, Limits, Rgb, RgbImage,
    codecs::jpeg::JpegEncoder, imageops::FilterType,
};
use serde::Serialize;
use sqlx::SqlitePool;
use url::Url;

use crate::{
    destinations::Compiled,
    outbound::{Failure, Fetcher, ImageFailure, Page, decode_entities, find_ignore_case},
};

/// Largest stored thumbnail. The migration enforces the same bound.
pub(crate) const MAX_STORED_BYTES: usize = 48 * 1024;
/// Bounding box for `og:image` and `twitter:image` thumbnails.
const IMAGE_BOX: (u32, u32) = (480, 270);
/// Bounding box for icons.
const ICON_SIDE: u32 = 64;
/// Smaller images, such as tracking pixels, are not previews.
const MIN_SIDE: u32 = 16;
/// Decoder limits against decompression bombs: at most 4096 pixels per side and 64 MiB for the
/// decoded image.
const MAX_SOURCE_SIDE: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;
/// JPEG qualities tried in order until the thumbnail fits `MAX_STORED_BYTES`.
const QUALITIES: [u8; 4] = [82, 70, 55, 40];
/// Image requests per preview, across all candidates.
const MAX_ATTEMPTS: usize = 3;
/// Tags examined in a document head, and attributes kept per tag.
const MAX_TAGS: usize = 1024;
const MAX_ATTRIBUTES: usize = 32;
const MAX_URL: usize = 4096;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// The page's `og:image` or `twitter:image`.
    Image,
    /// The page icon.
    Icon,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Icon => "icon",
        }
    }
}

/// A candidate image URL from a page, in preference order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) url: Url,
    pub(crate) kind: Kind,
}

/// A re-encoded JPEG thumbnail.
#[derive(Debug)]
pub(crate) struct Thumbnail {
    pub(crate) kind: Kind,
    pub(crate) bytes: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessError {
    /// Not a PNG, JPEG, WebP, or GIF image, or not decodable. SVG is always refused.
    Unsupported,
    /// The image exceeds the decoder limits, or its thumbnail exceeds the stored size cap.
    TooLarge,
    /// Smaller than 16 pixels on a side.
    TooSmall,
}

/// Image and icon URLs that a page head offers, resolved against the final page URL. Only
/// http and https URLs are kept. `/favicon.ico` is always the last candidate.
pub(crate) fn candidates(head: &[u8], page: &Url) -> Vec<Candidate> {
    let (mut og, mut twitter, mut touch, mut icon) = (None, None, None, None);
    for tag in Tags::new(head).take(MAX_TAGS) {
        match tag.name.as_str() {
            "meta" => {
                let Some(content) = tag.attr("content") else {
                    continue;
                };
                let key = tag
                    .attr("property")
                    .or_else(|| tag.attr("name"))
                    .map(str::to_ascii_lowercase);
                match key.as_deref() {
                    Some("og:image" | "og:image:url" | "og:image:secure_url") if og.is_none() => {
                        og = resolve(page, content);
                    }
                    Some("twitter:image" | "twitter:image:src") if twitter.is_none() => {
                        twitter = resolve(page, content);
                    }
                    _ => {}
                }
            }
            "link" => {
                let (Some(rel), Some(href)) = (tag.attr("rel"), tag.attr("href")) else {
                    continue;
                };
                if tag
                    .attr("type")
                    .is_some_and(|value| value.to_ascii_lowercase().contains("svg"))
                {
                    continue;
                }
                let Some(url) = resolve(page, href)
                    .filter(|url| !url.path().to_ascii_lowercase().ends_with(".svg"))
                else {
                    continue;
                };
                let rel = rel.to_ascii_lowercase();
                let tokens: Vec<&str> = rel.split_ascii_whitespace().collect();
                if tokens
                    .iter()
                    .any(|token| token.starts_with("apple-touch-icon"))
                {
                    touch.get_or_insert(url);
                } else if tokens.contains(&"icon") {
                    icon.get_or_insert(url);
                }
            }
            _ => {}
        }
    }
    let favicon = page.join("/favicon.ico").ok();
    let mut list: Vec<Candidate> = Vec::new();
    for (url, kind) in [
        (og, Kind::Image),
        (twitter, Kind::Image),
        (touch.or(icon), Kind::Icon),
        (favicon, Kind::Icon),
    ] {
        if let Some(url) = url
            && !list.iter().any(|candidate| candidate.url == url)
        {
            list.push(Candidate { url, kind });
        }
    }
    list
}

fn resolve(page: &Url, value: &str) -> Option<Url> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_URL {
        return None;
    }
    let mut url = page.join(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.as_str().len() > MAX_URL {
        return None;
    }
    url.set_fragment(None);
    Some(url)
}

struct Tag {
    name: String,
    attributes: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Start tags in a document head, in order. Comments and the contents of raw text elements are
/// skipped, and the scan ends at `</head>` or `<body>`. It reads bytes only and never allocates
/// more than one tag at a time.
struct Tags<'a> {
    html: &'a [u8],
    at: usize,
}

impl<'a> Tags<'a> {
    fn new(html: &'a [u8]) -> Self {
        Self { html, at: 0 }
    }
}

impl Iterator for Tags<'_> {
    type Item = Tag;

    fn next(&mut self) -> Option<Tag> {
        loop {
            let open = self.html.get(self.at..)?.iter().position(|b| *b == b'<')?;
            self.at += open;
            let rest = &self.html[self.at..];
            if rest.starts_with(b"<!--") {
                let end = find_ignore_case(&rest[4..], b"-->")?;
                self.at += 4 + end + 3;
                continue;
            }
            let closing = rest.get(1) == Some(&b'/');
            let start = if closing { 2 } else { 1 };
            let mut index = start;
            while rest
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            {
                index += 1;
            }
            let name = String::from_utf8_lossy(&rest[start..index]).to_ascii_lowercase();
            if name.is_empty() {
                self.at += 1;
                continue;
            }
            if closing {
                if name == "head" {
                    return None;
                }
                self.at += index;
                continue;
            }
            if name == "body" {
                return None;
            }
            let (attributes, end) = attributes(rest, index)?;
            self.at += end;
            if matches!(
                name.as_str(),
                "script" | "style" | "title" | "textarea" | "template"
            ) {
                let close = format!("</{name}");
                let skip = find_ignore_case(&self.html[self.at..], close.as_bytes())?;
                self.at += skip;
                continue;
            }
            return Some(Tag { name, attributes });
        }
    }
}

/// Parses attributes from `index` to the closing `>`. Returns them and the offset after `>`.
fn attributes(tag: &[u8], mut index: usize) -> Option<(Vec<(String, String)>, usize)> {
    let mut attributes = Vec::new();
    let text = |bytes: &[u8]| decode_entities(&String::from_utf8_lossy(bytes));
    loop {
        while tag
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/')
        {
            index += 1;
        }
        if *tag.get(index)? == b'>' {
            return Some((attributes, index + 1));
        }
        let start = index;
        while tag
            .get(index)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'>' | b'/'))
        {
            index += 1;
        }
        let name = String::from_utf8_lossy(&tag[start..index]).to_ascii_lowercase();
        while tag.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let mut value = String::new();
        if tag.get(index) == Some(&b'=') {
            index += 1;
            while tag.get(index).is_some_and(u8::is_ascii_whitespace) {
                index += 1;
            }
            match *tag.get(index)? {
                quote @ (b'"' | b'\'') => {
                    let length = tag[index + 1..].iter().position(|byte| *byte == quote)?;
                    value = text(&tag[index + 1..index + 1 + length]);
                    index += length + 2;
                }
                _ => {
                    let start = index;
                    while tag
                        .get(index)
                        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
                    {
                        index += 1;
                    }
                    value = text(&tag[start..index]);
                }
            }
        }
        if !name.is_empty() && attributes.len() < MAX_ATTRIBUTES {
            attributes.push((name, value));
        }
    }
}

fn sniff(data: &[u8]) -> Option<ImageFormat> {
    if data.starts_with(PNG_SIGNATURE) {
        Some(ImageFormat::Png)
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

/// The largest PNG image inside an ICO container, which is what most `/favicon.ico` files hold
/// today. BMP entries are not decoded.
fn ico_png(data: &[u8]) -> Option<&[u8]> {
    if data.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = usize::from(u16::from_le_bytes([data[4], *data.get(5)?]));
    if count == 0 || count > 64 {
        return None;
    }
    let mut best: Option<(u32, &[u8])> = None;
    for index in 0..count {
        let entry = data.get(6 + index * 16..6 + index * 16 + 16)?;
        let side = if entry[0] == 0 {
            256
        } else {
            u32::from(entry[0])
        };
        let size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        let Some(image) = offset
            .checked_add(size)
            .and_then(|end| data.get(offset..end))
        else {
            continue;
        };
        if image.starts_with(PNG_SIGNATURE) && best.is_none_or(|(largest, _)| side > largest) {
            best = Some((side, image));
        }
    }
    best.map(|(_, image)| image)
}

/// Decodes untrusted image bytes under strict limits and returns a re-encoded JPEG thumbnail.
/// CPU-bound: call it on a blocking thread.
pub(crate) fn thumbnail(data: &[u8], kind: Kind) -> Result<Thumbnail, ProcessError> {
    let (data, format) = match sniff(data) {
        Some(format) => (data, format),
        None => (
            ico_png(data).ok_or(ProcessError::Unsupported)?,
            ImageFormat::Png,
        ),
    };
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    // A GIF or animated WebP decodes to its first frame.
    let image = reader.decode().map_err(|error| match error {
        ImageError::Limits(_) => ProcessError::TooLarge,
        _ => ProcessError::Unsupported,
    })?;
    if image.width() < MIN_SIDE || image.height() < MIN_SIDE {
        return Err(ProcessError::TooSmall);
    }
    let (width, height) = match kind {
        Kind::Image => IMAGE_BOX,
        Kind::Icon => (ICON_SIDE, ICON_SIDE),
    };
    let image = if image.width() > width || image.height() > height {
        image.resize(width, height, FilterType::Triangle)
    } else {
        image
    };
    let pixels = flatten(&image);
    for quality in QUALITIES {
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, quality)
            .encode(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|_| ProcessError::Unsupported)?;
        if bytes.len() <= MAX_STORED_BYTES {
            return Ok(Thumbnail {
                kind,
                bytes,
                width: pixels.width(),
                height: pixels.height(),
            });
        }
    }
    Err(ProcessError::TooLarge)
}

/// Composites transparency onto white, since JPEG has no alpha channel.
fn flatten(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
        let alpha = u32::from(alpha);
        let blend = |channel: u8| {
            ((u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255).min(255) as u8
        };
        Rgb([blend(red), blend(green), blend(blue)])
    })
}

/// The result of one preview attempt.
#[derive(Debug)]
pub(crate) enum Outcome {
    Ready {
        thumbnail: Thumbnail,
        /// Scheme, host, and port of the image URL.
        origin: String,
    },
    /// The page offers no usable image.
    None,
    Failed(&'static str),
    Skipped(&'static str),
}

pub(crate) fn failure_code(failure: Failure) -> &'static str {
    match failure {
        Failure::Blocked => "blocked",
        Failure::Busy => "busy",
        Failure::Dns => "dns",
        Failure::Timeout => "timeout",
        Failure::Connection => "connection",
        Failure::Protocol => "protocol",
        Failure::NotHtml => "not_html",
    }
}

/// Fetches and processes the page's candidate images in order until one succeeds. At most
/// three image requests are sent. Each candidate URL must also pass the destination policy.
pub(crate) async fn build(fetcher: &Fetcher, policy: &Compiled, page: &Page) -> Outcome {
    // A failure that says something about the network or the image rather than its absence.
    let mut failure: Option<&'static str> = None;
    for candidate in candidates(&page.head, &page.url)
        .into_iter()
        .take(MAX_ATTEMPTS)
    {
        if !policy.permits(candidate.url.as_str()) {
            failure.get_or_insert("policy");
            continue;
        }
        let bytes = match fetcher.fetch_image(candidate.url.as_str()).await {
            Ok(bytes) => bytes,
            // A missing image or an unsupported type only means there is no preview here.
            Err(ImageFailure::Fetch(Failure::Protocol) | ImageFailure::Unsupported) => continue,
            Err(ImageFailure::TooLarge) => {
                failure.get_or_insert("too_large");
                continue;
            }
            Err(ImageFailure::Fetch(other)) => {
                failure.get_or_insert(failure_code(other));
                continue;
            }
        };
        let kind = candidate.kind;
        match tokio::task::spawn_blocking(move || thumbnail(&bytes, kind)).await {
            Ok(Ok(thumbnail)) => {
                return Outcome::Ready {
                    thumbnail,
                    origin: candidate.url.origin().ascii_serialization(),
                };
            }
            Ok(Err(ProcessError::TooLarge)) => {
                failure.get_or_insert("too_large");
            }
            Ok(Err(ProcessError::Unsupported | ProcessError::TooSmall)) => {}
            Err(_) => {
                failure.get_or_insert("unsupported");
            }
        }
    }
    match failure {
        Some(code) => Outcome::Failed(code),
        None => Outcome::None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequestError {
    Disabled,
    /// Title lookup is off, and previews come from its page request.
    TitleLookupRequired,
    Missing,
    Protected,
    Database,
}

impl From<sqlx::Error> for RequestError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

/// A pending preview and the destination it was requested for.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct Job {
    pub(crate) link_id: i64,
    pub(crate) revision: String,
    pub(crate) host: String,
    pub(crate) target: String,
}

/// The link `links` has no password or one-time protection.
macro_rules! not_protected {
    () => {
        "NOT EXISTS (
            SELECT 1 FROM link_protection AS protection WHERE protection.link_id = links.id
            AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL))"
    };
}

/// Queues a new fetch for one link and clears its current preview. A fetch that is already
/// pending is kept.
pub(crate) async fn request(pool: &SqlitePool, id: i64) -> Result<(), RequestError> {
    let protected = sqlx::query_scalar::<_, bool>(concat!(
        "SELECT NOT ",
        not_protected!(),
        " FROM links WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(RequestError::Missing)?;
    if protected {
        return Err(RequestError::Protected);
    }
    sqlx::query(concat!(
        "INSERT INTO link_previews (link_id) SELECT id FROM links WHERE id = ? AND ",
        not_protected!(),
        "
         ON CONFLICT (link_id) DO UPDATE SET
             revision = lower(hex(randomblob(16))), status = 'pending', failure = NULL,
             kind = NULL, mime = NULL, bytes = NULL, width = NULL, height = NULL,
             source_origin = NULL, requested_at = unixepoch(), fetched_at = NULL
         WHERE link_previews.status <> 'pending'"
    ))
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn job(pool: &SqlitePool, id: i64) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT preview.link_id, preview.revision, links.host, links.target
         FROM link_previews AS preview JOIN links ON links.id = preview.link_id
         WHERE preview.link_id = ? AND preview.status = 'pending' AND ",
        not_protected!(),
        ""
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The pending preview of one link, queued now if the link has never had one. A link whose
/// preview already finished is not fetched again until it is refreshed or its destination
/// changes.
pub(crate) async fn claim(pool: &SqlitePool, id: i64) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query(concat!(
        "INSERT INTO link_previews (link_id) SELECT id FROM links WHERE id = ? AND ",
        not_protected!(),
        "
         ON CONFLICT (link_id) DO NOTHING"
    ))
    .bind(id)
    .execute(pool)
    .await?;
    job(pool, id).await
}

/// The oldest pending preview, otherwise a new one for the newest link that has never had one.
/// Each call queues at most one link, so the backlog drains at the worker's pace.
pub(crate) async fn next_job(pool: &SqlitePool) -> Result<Option<Job>, sqlx::Error> {
    let pending: Option<i64> = sqlx::query_scalar(concat!(
        "SELECT preview.link_id FROM link_previews AS preview
         JOIN links ON links.id = preview.link_id
         WHERE preview.status = 'pending' AND ",
        not_protected!(),
        "
         ORDER BY preview.requested_at, preview.link_id LIMIT 1"
    ))
    .fetch_optional(pool)
    .await?;
    let id = match pending {
        Some(id) => Some(id),
        None => {
            sqlx::query_scalar(concat!(
                "INSERT INTO link_previews (link_id)
                 SELECT links.id FROM links
                 WHERE NOT EXISTS (SELECT 1 FROM link_previews AS preview
                                   WHERE preview.link_id = links.id)
                   AND ",
                not_protected!(),
                "
                 ORDER BY links.id DESC LIMIT 1
                 ON CONFLICT (link_id) DO NOTHING
                 RETURNING link_id"
            ))
            .fetch_optional(pool)
            .await?
        }
    };
    match id {
        Some(id) => job(pool, id).await,
        None => Ok(None),
    }
}

/// Records the outcome. A stale result, for a changed or newly protected link or a newer
/// request, is dropped. Returns whether it was recorded.
pub(crate) async fn finish(
    pool: &SqlitePool,
    job: &Job,
    outcome: Outcome,
) -> Result<bool, sqlx::Error> {
    let (status, failure, ready) = match &outcome {
        Outcome::Ready { thumbnail, origin } => ("ready", None, Some((thumbnail, origin))),
        Outcome::None => ("none", None, None),
        Outcome::Failed(code) => ("failed", Some(*code), None),
        Outcome::Skipped(code) => ("skipped", Some(*code), None),
    };
    let recorded = sqlx::query(concat!(
        "UPDATE link_previews SET status = ?, failure = ?, kind = ?, mime = ?, bytes = ?,
             width = ?, height = ?, source_origin = ?, fetched_at = unixepoch()
         WHERE link_id = ? AND revision = ? AND status = 'pending' AND EXISTS (
             SELECT 1 FROM links WHERE id = ? AND host = ? AND target = ? AND ",
        not_protected!(),
        "
         )"
    ))
    .bind(status)
    .bind(failure)
    .bind(ready.map(|(thumbnail, _)| thumbnail.kind.as_str()))
    .bind(ready.map(|_| "image/jpeg"))
    .bind(ready.map(|(thumbnail, _)| thumbnail.bytes.as_slice()))
    .bind(ready.map(|(thumbnail, _)| i64::from(thumbnail.width)))
    .bind(ready.map(|(thumbnail, _)| i64::from(thumbnail.height)))
    .bind(ready.map(|(_, origin)| origin.as_str()))
    .bind(job.link_id)
    .bind(&job.revision)
    .bind(job.link_id)
    .bind(&job.host)
    .bind(&job.target)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(recorded == 1)
}

/// Drops fetches that were pending in a restored snapshot. The restore scrub calls it. A
/// restored installation queues its own fetches once previews are on there.
pub(crate) async fn discard_pending<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
) -> Result<u64, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM link_previews WHERE status = 'pending'")
            .execute(executor)
            .await?
            .rows_affected(),
    )
}

/// The stored thumbnail of a link within `hosts`, as its media type and bytes.
pub(crate) async fn image(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
) -> Result<Option<(String, Vec<u8>)>, sqlx::Error> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT preview.mime, preview.bytes FROM link_previews AS preview
         JOIN links ON links.id = preview.link_id
         WHERE preview.status = 'ready' AND preview.link_id = ",
    );
    query.push_bind(id);
    crate::store::push_host_scope(&mut query, hosts);
    query
        .build_query_as::<(String, Vec<u8>)>()
        .fetch_optional(pool)
        .await
}

/// A minimal HTTP/1.1 server on a loopback port for unit tests. Each route returns a fixed
/// status, content type, extra headers, and body; unknown paths return 404.
#[cfg(test)]
pub(crate) mod test_server {
    use std::{collections::HashMap, sync::Arc};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::Mutex,
    };

    #[derive(Clone)]
    pub(crate) struct Route {
        pub(crate) status: u16,
        pub(crate) headers: Vec<(String, String)>,
        pub(crate) body: Vec<u8>,
    }

    impl Route {
        pub(crate) fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
            Self {
                status: 200,
                headers: vec![("Content-Type".into(), content_type.into())],
                body: body.into(),
            }
        }

        pub(crate) fn redirect(location: &str) -> Self {
            Self {
                status: 302,
                headers: vec![("Location".into(), location.into())],
                body: Vec::new(),
            }
        }
    }

    pub(crate) struct Server {
        pub(crate) port: u16,
        /// Paths requested so far, in order.
        pub(crate) requests: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        pub(crate) fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.port)
        }
    }

    pub(crate) async fn start(routes: Vec<(&str, Route)>) -> Server {
        let routes: Arc<HashMap<String, Route>> = Arc::new(
            routes
                .into_iter()
                .map(|(path, route)| (path.to_owned(), route))
                .collect(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let routes = routes.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0; 4096];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => request.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                    log.lock().await.push(path.clone());
                    let route = routes.get(&path).cloned().unwrap_or(Route {
                        status: 404,
                        headers: vec![("Content-Type".into(), "text/plain".into())],
                        body: b"missing".to_vec(),
                    });
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                        route.status,
                        route.body.len()
                    );
                    for (name, value) in &route.headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(&route.body).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Server { port, requests }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{test_server::Route, *};

    /// A PNG of `width` x `height` with a text chunk, for metadata checks.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .add_text_chunk("Comment".into(), "synthetic-private-note".into())
                .unwrap();
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = (0..width * height)
                .flat_map(|index| {
                    let value = (index % 251) as u8;
                    [
                        value,
                        255 - value,
                        128,
                        if index % 7 == 0 { 0 } else { 255 },
                    ]
                })
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        bytes
    }

    /// A JPEG with an EXIF segment that names a synthetic camera and location.
    fn jpeg_with_exif(width: u32, height: u32) -> Vec<u8> {
        let pixels = RgbImage::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, 99]));
        let mut plain = Vec::new();
        JpegEncoder::new_with_quality(&mut plain, 90)
            .encode(pixels.as_raw(), width, height, ExtendedColorType::Rgb8)
            .unwrap();
        let payload = b"Exif\0\0SyntheticCamera GPS 12.34N";
        let length = (payload.len() + 2) as u16;
        let mut bytes = plain[..2].to_vec();
        bytes.extend_from_slice(&[0xff, 0xe1]);
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&plain[2..]);
        bytes
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A tiny PNG whose header claims `width` x `height` 16-bit RGBA pixels.
    fn png_bomb(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            bytes.extend_from_slice(&body);
            bytes.extend_from_slice(&crc32(&body).to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[16, 6, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        chunk(b"IEND", &[]);
        bytes
    }

    fn page(url: &str) -> Url {
        Url::parse(url).unwrap()
    }

    fn urls(list: &[Candidate]) -> Vec<(&str, Kind)> {
        list.iter()
            .map(|candidate| (candidate.url.as_str(), candidate.kind))
            .collect()
    }

    #[test]
    fn og_and_twitter_images_resolve_against_the_final_page_url() {
        let head = br#"<!doctype html><html><head>
            <!-- <meta property="og:image" content="/commented.png"> -->
            <title>Has <meta property="og:image" content="/in-title.png"></title>
            <script>var x = '<meta property="og:image" content="/in-script.png">';</script>
            <META PROPERTY="og:image" CONTENT="../img/card.png?sig=a&amp;v=2#frag">
            <meta property="og:image" content="/second.png">
            <meta name=twitter:image content=https://cdn.example/t.jpg>
            <link rel="mask-icon" href="/mask.svg">
            <link rel="icon" type="image/svg+xml" href="/icon.svg">
            <link rel="icon" href="/favicon-32.png">
            <link rel="apple-touch-icon" href="/touch.png">
            </head><body><meta property="og:image" content="/body.png">"#;
        let list = candidates(head, &page("https://example.test/docs/page/?q=1"));
        assert_eq!(
            urls(&list),
            [
                (
                    "https://example.test/docs/img/card.png?sig=a&v=2",
                    Kind::Image
                ),
                ("https://cdn.example/t.jpg", Kind::Image),
                ("https://example.test/touch.png", Kind::Icon),
                ("https://example.test/favicon.ico", Kind::Icon),
            ]
        );
        // Entities, single quotes, and secure_url variants are understood.
        let list = candidates(
            b"<meta content='//img.example/a&#x2e;png' property='og:image:secure_url'>",
            &page("https://example.test/"),
        );
        assert_eq!(list[0].url.as_str(), "https://img.example/a.png");
    }

    #[test]
    fn favicon_fallback_and_non_http_urls() {
        let list = candidates(
            br#"<meta property="og:image" content="data:image/png;base64,AAAA">
                <meta property="twitter:image" content="javascript:alert(1)">
                <link rel="shortcut icon" href="ftp://example.test/x.png">"#,
            &page("http://example.test/a/b"),
        );
        assert_eq!(
            urls(&list),
            [("http://example.test/favicon.ico", Kind::Icon)]
        );
        let list = candidates(
            br#"<link rel="shortcut icon" href="/static/icon.png">"#,
            &page("https://example.test/"),
        );
        assert_eq!(
            urls(&list),
            [
                ("https://example.test/static/icon.png", Kind::Icon),
                ("https://example.test/favicon.ico", Kind::Icon)
            ]
        );
        // Unterminated markup ends the scan without panicking.
        for head in [
            &b"<meta property=\"og:image"[..],
            b"<!-- never closed",
            b"<script>never closed",
            b"<",
            b"<meta =x property=og:image content=/a.png>",
        ] {
            let _ = candidates(head, &page("https://example.test/"));
        }
    }

    #[test]
    fn thumbnails_are_bounded_jpegs_without_metadata() {
        let image = thumbnail(&png(1200, 600), Kind::Image).unwrap();
        assert_eq!((image.width, image.height), (480, 240));
        assert!(image.bytes.starts_with(&[0xff, 0xd8, 0xff]));
        assert!(image.bytes.len() <= MAX_STORED_BYTES);
        let icon = thumbnail(&png(180, 180), Kind::Icon).unwrap();
        assert_eq!((icon.width, icon.height), (64, 64));
        let small = thumbnail(&png(32, 20), Kind::Image).unwrap();
        assert_eq!((small.width, small.height), (32, 20));

        let exif = jpeg_with_exif(300, 200);
        assert!(find_ignore_case(&exif, b"SyntheticCamera").is_some());
        for (source, kind) in [(exif, Kind::Image), (png(100, 100), Kind::Icon)] {
            let output = thumbnail(&source, kind).unwrap().bytes;
            for marker in [&b"Exif"[..], b"SyntheticCamera", b"synthetic-private-note"] {
                assert!(find_ignore_case(&output, marker).is_none());
            }
            // Only SOI, the JFIF header, tables, frame, scan, and EOI segments: no APP1-APP15
            // (EXIF, XMP, ICC) and no comment segment.
            let mut index = 2;
            while index + 4 <= output.len() {
                assert_eq!(output[index], 0xff);
                let marker = output[index + 1];
                assert!(
                    !(0xe1..=0xef).contains(&marker) && marker != 0xfe,
                    "{marker:x}"
                );
                if marker == 0xda {
                    break;
                }
                let length =
                    usize::from(u16::from_be_bytes([output[index + 2], output[index + 3]]));
                index += 2 + length;
            }
        }
    }

    #[test]
    fn decoder_limits_refuse_bombs_svg_and_unknown_formats() {
        assert_eq!(
            thumbnail(&png_bomb(20_000, 20_000), Kind::Image).err(),
            Some(ProcessError::TooLarge)
        );
        // Within the side limit but over the 64 MiB allocation budget (4000 x 4000 x 8 bytes).
        assert_eq!(
            thumbnail(&png_bomb(4000, 4000), Kind::Image).err(),
            Some(ProcessError::TooLarge)
        );
        assert_eq!(
            thumbnail(&png(4097, 1), Kind::Image).err(),
            Some(ProcessError::TooLarge)
        );
        for source in [
            &br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#[..],
            br#"<?xml version="1.0"?><svg/>"#,
            b"BM\x00\x00",
            b"",
            b"\x89PNG\r\n\x1a\ntruncated",
        ] {
            assert_eq!(
                thumbnail(source, Kind::Icon).err(),
                Some(ProcessError::Unsupported)
            );
        }
        assert_eq!(
            thumbnail(&png(8, 8), Kind::Icon).err(),
            Some(ProcessError::TooSmall)
        );
    }

    #[test]
    fn ico_files_yield_their_largest_png_entry() {
        let small = png(16, 16);
        let large = png(48, 48);
        let mut ico = vec![0, 0, 1, 0, 3, 0];
        let header = 6 + 3 * 16;
        let entries = [
            (16u8, small.len(), header),
            (48u8, large.len(), header + small.len()),
            // A broken entry pointing outside the file is ignored.
            (64u8, 10_000, 1_000_000),
        ];
        for (side, size, offset) in entries {
            ico.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&(size as u32).to_le_bytes());
            ico.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        ico.extend_from_slice(&small);
        ico.extend_from_slice(&large);
        let icon = thumbnail(&ico, Kind::Icon).unwrap();
        assert_eq!((icon.width, icon.height), (48, 48));
        // An ICO with only BMP entries is not decoded.
        let mut bmp = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&22u32.to_le_bytes());
        bmp.extend_from_slice(&[0; 40]);
        assert_eq!(
            thumbnail(&bmp, Kind::Icon).err(),
            Some(ProcessError::Unsupported)
        );
    }

    fn html(body: &str) -> Route {
        Route::ok("text/html; charset=utf-8", body.as_bytes().to_vec())
    }

    async fn page_from(server: &test_server::Server, fetcher: &Fetcher, path: &str) -> Page {
        fetcher.fetch_page(&server.url(path), true).await.unwrap()
    }

    #[tokio::test]
    async fn image_fetches_follow_the_outbound_policy() {
        let card = png(640, 360);
        let server = test_server::start(vec![
            (
                "/start",
                Route::redirect("/article/"),
            ),
            (
                "/article/",
                html(
                    r#"<head><title>Article</title><meta property="og:image" content="card.png"></head>"#,
                ),
            ),
            ("/article/card.png", Route::ok("image/png", card.clone())),
            (
                "/private",
                html(
                    r#"<meta property="og:image" content="http://127.0.0.1/card.png">
                       <meta name="twitter:image" content="http://10.0.0.8/card.png">
                       <link rel="icon" href="/redirected.png">"#,
                ),
            ),
            ("/redirected.png", Route::redirect("http://192.168.1.4/i.png")),
            ("/loop.png", Route::redirect("http://127.0.0.1:1/favicon.ico")),
            (
                "/svg",
                html(r#"<meta property="og:image" content="/drawing">"#),
            ),
            (
                "/drawing",
                Route::ok("image/svg+xml", br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec()),
            ),
            ("/huge", html(r#"<meta property="og:image" content="/huge.png">"#)),
            (
                "/huge.png",
                Route::ok("image/png", vec![0; crate::outbound::MAX_IMAGE_BYTES + 1]),
            ),
        ])
        .await;
        let fetcher = Fetcher::with_test_port(server.port);
        let policy = Compiled::permissive();

        let page = page_from(&server, &fetcher, "/start").await;
        assert_eq!(page.title.as_deref(), Some("Article"));
        assert_eq!(page.url.as_str(), server.url("/article/"));
        match build(&fetcher, &policy, &page).await {
            Outcome::Ready { thumbnail, origin } => {
                assert_eq!(thumbnail.kind, Kind::Image);
                assert_eq!((thumbnail.width, thumbnail.height), (480, 270));
                assert_eq!(origin, server.url(""));
            }
            other => panic!("{other:?}"),
        }

        // Private and loopback image URLs, and redirects to them, are refused before any
        // connection to those addresses.
        let page = page_from(&server, &fetcher, "/private").await;
        assert!(matches!(
            build(&fetcher, &policy, &page).await,
            Outcome::Failed("blocked")
        ));
        for url in [
            "http://127.0.0.1/card.png",
            "http://10.0.0.8/card.png",
            &server.url("/redirected.png"),
            &server.url("/loop.png"),
        ] {
            assert_eq!(
                fetcher.fetch_image(url).await,
                Err(ImageFailure::Fetch(Failure::Blocked)),
                "{url}"
            );
        }

        assert_eq!(
            fetcher.fetch_image(&server.url("/drawing")).await,
            Err(ImageFailure::Unsupported)
        );
        let page = page_from(&server, &fetcher, "/svg").await;
        assert!(matches!(
            build(&fetcher, &policy, &page).await,
            Outcome::None
        ));
        assert_eq!(
            fetcher.fetch_image(&server.url("/huge.png")).await,
            Err(ImageFailure::TooLarge)
        );
        let page = page_from(&server, &fetcher, "/huge").await;
        assert!(matches!(
            build(&fetcher, &policy, &page).await,
            Outcome::Failed("too_large")
        ));
        // The destination policy applies to image URLs too.
        let blocking = Compiled::blocking("127.0.0.1");
        let page = page_from(&server, &fetcher, "/article/").await;
        assert!(matches!(
            build(&fetcher, &blocking, &page).await,
            Outcome::Failed("policy")
        ));
    }
}
