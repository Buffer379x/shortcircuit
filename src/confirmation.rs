use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{auth::lowercase_hex, redirect::Challenge};

const VALID_SECONDS: u64 = 5 * 60;
const MAX_FORM_BYTES: usize = 4096;
const FORM_TIMEOUT: Duration = Duration::from_secs(5);
static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();

type LinkMac = Hmac<Sha256>;

#[derive(Clone, Copy)]
pub(crate) struct Action<'a> {
    pub(crate) link: Challenge,
    pub(crate) origin: &'a str,
    pub(crate) path_and_query: &'a str,
}

impl Action<'_> {
    fn now() -> Result<u64, StatusCode> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
    }

    fn signature(self, expires: u64) -> Result<String, StatusCode> {
        let key = KEY
            .get_or_init(|| {
                let mut key = [0; 32];
                getrandom::fill(&mut key).ok().map(|()| key)
            })
            .as_ref()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let mut mac = LinkMac::new_from_slice(key).expect("HMAC accepts a 32-byte key");
        mac.update(b"shortcircuit/protected-link/v1\0");
        mac.update(&self.link.id.to_be_bytes());
        mac.update(&self.link.revision.to_be_bytes());
        mac.update(&expires.to_be_bytes());
        mac.update(self.origin.as_bytes());
        mac.update(b"\0");
        mac.update(self.path_and_query.as_bytes());
        Ok(lowercase_hex(&mac.finalize().into_bytes()))
    }

    fn issue(self) -> Result<String, StatusCode> {
        let expires = Self::now()? + VALID_SECONDS;
        Ok(format!("{expires}.{}", self.signature(expires)?))
    }

    fn verify(self, token: &str) -> Result<(), StatusCode> {
        let (expires, supplied) = token.split_once('.').ok_or(StatusCode::FORBIDDEN)?;
        let expires = expires.parse::<u64>().map_err(|_| StatusCode::FORBIDDEN)?;
        let now = Self::now()?;
        if expires <= now || expires > now + VALID_SECONDS || supplied.len() != 64 {
            return Err(StatusCode::FORBIDDEN);
        }
        let actual = self.signature(expires)?;
        if !bool::from(actual.as_bytes().ct_eq(supplied.as_bytes())) {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(())
    }

    pub(crate) async fn submit(
        self,
        headers: &HeaderMap,
        body: Body,
    ) -> Result<String, StatusCode> {
        let mut origins = headers.get_all(header::ORIGIN).iter();
        if origins.next().and_then(|value| value.to_str().ok()) != Some(self.origin)
            || origins.next().is_some()
        {
            return Err(StatusCode::FORBIDDEN);
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| {
                value
                    .trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            });
        if !content_type {
            return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        let bytes = tokio::time::timeout(FORM_TIMEOUT, to_bytes(body, MAX_FORM_BYTES))
            .await
            .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        let mut challenge = None;
        let mut password = None;
        for (name, value) in url::form_urlencoded::parse(&bytes) {
            match name.as_ref() {
                "challenge" if challenge.is_none() => challenge = Some(value),
                "password" if password.is_none() => password = Some(value),
                _ => return Err(StatusCode::BAD_REQUEST),
            }
        }
        self.verify(challenge.as_deref().ok_or(StatusCode::FORBIDDEN)?)?;
        Ok(password.unwrap_or_default().into_owned())
    }

    pub(crate) fn page(self, language: Option<&str>, wrong_password: bool) -> Response {
        let Ok(token) = self.issue() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let copy = Strings::for_language(language);
        let action = escape(&format!("{}{}", self.origin, self.path_and_query));
        let host = self
            .origin
            .split_once("://")
            .map_or(self.origin, |(_, host)| host);
        let path = self
            .path_and_query
            .split_once('?')
            .map_or(self.path_and_query, |(path, _)| path);
        let short_url = escape(&format!("{host}{path}"));
        let (title, content, button) = if self.link.password_required {
            let (invalid, feedback) = if wrong_password {
                (
                    " aria-invalid=\"true\" aria-describedby=\"password-error\"",
                    format!(
                        "<p class=\"error\" id=\"password-error\" role=\"alert\">{ALERT}{}</p>",
                        copy.password_error
                    ),
                )
            } else {
                ("", String::new())
            };
            let (before, after) = copy.password_text;
            (
                copy.password_title,
                format!(
                    "<p class=\"muted\">{before}<span class=\"mono\">{short_url}</span>{after}</p>\
                     <form method=\"post\" action=\"{action}\"><input type=\"hidden\" name=\"challenge\" value=\"{token}\">\
                     <div class=\"field\"><label for=\"password\">{}</label>\
                     <input id=\"password\" type=\"password\" name=\"password\" autocomplete=\"current-password\" required autofocus{invalid}>\
                     {feedback}</div>",
                    copy.password_label
                ),
                copy.password_button,
            )
        } else {
            (
                copy.once_title,
                format!(
                    "<p class=\"muted\">{}</p><form method=\"post\" action=\"{action}\">\
                     <input type=\"hidden\" name=\"challenge\" value=\"{token}\">",
                    copy.once_text
                ),
                copy.once_button,
            )
        };
        let status = if wrong_password {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::OK
        };
        let content = format!(
            "<h1>{title}</h1>{content}<button class=\"btn primary\" type=\"submit\">{button}</button></form>"
        );
        let mut response = html(status, copy, title, &content, true);
        response.headers_mut().insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin"),
        );
        response
    }
}

/// A public page that explains why a request has no destination.
pub(crate) enum Notice {
    /// No link matches the path. `home` offers the domain root when a root destination redirects.
    Missing { home: Option<Home> },
    /// The host is not an enabled public domain.
    UnknownHost,
    /// The admin listener received an address other than `ADMIN_ORIGIN`. The page names neither
    /// the configured address nor the product, so it reveals nothing about the installation.
    AdminAddress,
}

pub(crate) struct Home {
    /// The public domain's own root URL, which redirects to the root destination.
    pub(crate) href: String,
    /// The host the root destination points to.
    pub(crate) label: String,
}

/// Renders a notice for GET and POST. HEAD keeps the bare status without a body.
pub(crate) fn notice(
    method: &Method,
    status: StatusCode,
    notice: Notice,
    language: Option<&str>,
) -> Response {
    if method == Method::HEAD {
        return status.into_response();
    }
    let copy = Strings::for_language(language);
    let code = status.as_u16();
    let (title, text, home, footer) = match notice {
        Notice::Missing { home } => (copy.missing_title, copy.missing_text, home, true),
        Notice::UnknownHost => (copy.unknown_title, copy.unknown_text, None, true),
        Notice::AdminAddress => (copy.admin_title, copy.admin_text, None, false),
    };
    let link = home.map_or_else(String::new, |home| {
        format!(
            "<a class=\"btn\" href=\"{}\">{}{}</a>",
            escape(&home.href),
            copy.home,
            escape(&home.label)
        )
    });
    let content =
        format!("<p class=\"code\">{code}</p><h1>{title}</h1><p class=\"muted\">{text}</p>{link}");
    html(status, copy, title, &content, footer)
}

/// Visitor-facing strings. English is the default; a first preferred language of `de` selects German.
struct Strings {
    lang: &'static str,
    password_title: &'static str,
    password_text: (&'static str, &'static str),
    password_label: &'static str,
    password_error: &'static str,
    password_button: &'static str,
    once_title: &'static str,
    once_text: &'static str,
    once_button: &'static str,
    missing_title: &'static str,
    missing_text: &'static str,
    home: &'static str,
    unknown_title: &'static str,
    unknown_text: &'static str,
    admin_title: &'static str,
    admin_text: &'static str,
    footer: &'static str,
}

const ENGLISH: Strings = Strings {
    lang: "en",
    password_title: "This link is protected",
    password_text: ("Enter the password you received with ", "."),
    password_label: "Password",
    password_error: "That password is not correct. Try again.",
    password_button: "Continue",
    once_title: "Open this link once",
    once_text: "This link works for a single visit. After you continue, it can’t be used again, not even by you.",
    once_button: "Continue to the page",
    missing_title: "This short link doesn’t exist",
    missing_text: "Check the address for typos. Short links are case-sensitive.",
    home: "Go to ",
    unknown_title: "Nothing is served here",
    unknown_text: "This domain is not set up for short links.",
    admin_title: "The administration isn’t available at this address",
    admin_text: "Open the address that is configured for the administration.",
    footer: "Served by ShortCircuit",
};

const GERMAN: Strings = Strings {
    lang: "de",
    password_title: "Dieser Link ist geschützt",
    password_text: ("Gib das Passwort ein, das du mit ", " erhalten hast."),
    password_label: "Passwort",
    password_error: "Das Passwort ist nicht korrekt. Versuche es erneut.",
    password_button: "Weiter",
    once_title: "Diesen Link einmal öffnen",
    once_text: "Dieser Link funktioniert für einen einzigen Besuch. Danach kann ihn niemand mehr verwenden, auch du nicht.",
    once_button: "Weiter zur Seite",
    missing_title: "Diesen Kurzlink gibt es nicht",
    missing_text: "Prüfe die Adresse auf Tippfehler. Kurzlinks unterscheiden zwischen Groß- und Kleinschreibung.",
    home: "Weiter zu ",
    unknown_title: "Hier wird nichts bereitgestellt",
    unknown_text: "Diese Domain ist nicht für Kurzlinks eingerichtet.",
    admin_title: "Die Verwaltung ist unter dieser Adresse nicht erreichbar",
    admin_text: "Öffne die Adresse, die für die Verwaltung eingerichtet ist.",
    footer: "Bereitgestellt von ShortCircuit",
};

impl Strings {
    fn for_language(language: Option<&str>) -> &'static Self {
        let german = language
            .and_then(|value| value.split(['-', ',', ';']).next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("de"));
        if german { &GERMAN } else { &ENGLISH }
    }
}

/// The Polar Signal mark, colored by the stylesheet.
const MARK: &str = "<svg class=\"mark\" viewBox=\"0 0 64 64\" aria-hidden=\"true\">\
    <path d=\"M33.07 10H20A10 10 0 0 0 10 20v7a3.5 3.5 0 0 0 7 0v-7a3 3 0 0 1 3-3h7.06z\"/>\
    <path d=\"M30.93 54H44a10 10 0 0 0 10-10v-7a3.5 3.5 0 0 0-7 0v7a3 3 0 0 1-3 3h-7.06z\"/>\
    <path d=\"M39 8 15 36h15l-5 20 24-28H34z\"/></svg>";

const ALERT: &str = "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><circle cx=\"12\" cy=\"12\" r=\"10\"/>\
    <line x1=\"12\" x2=\"12\" y1=\"8\" y2=\"12\"/><line x1=\"12\" x2=\"12.01\" y1=\"16\" y2=\"16\"/></svg>";

/// The only stylesheet of the public pages. The CSP allows exactly this text by its hash, so it
/// must stay static. The colors follow the Polar Signal tokens. Geist is not served publicly, and
/// named system fonts come before `system-ui`, which some Linux setups map to a CJK face.
const STYLE: &str = ":root{color-scheme:dark;--bg:#101115;--surface:#1c1e24;--raised:#242731;\
--hover:#292d36;--sunken:#16181d;--border:#333640;--subtle:#272a32;--control:#727788;--text:#f6f6fa;\
--secondary:#c3c5cf;--muted:#969aa9;--accent:#ff79c6;--accent-hover:#ff94d2;--accent-active:#e963af;\
--accent-soft:#302333;--on-accent:#101115;--error:#ff6b61;--error-soft:#3a1e1f;--h:32px;--h-lg:40px}\
@media (prefers-color-scheme:light){:root{color-scheme:light;--bg:#f5f5f8;--surface:#fff;--raised:#fff;\
--hover:#f0eff5;--sunken:#f8f8fb;--border:#dadbe3;--subtle:#e7e7ee;--control:#858998;--text:#20222a;\
--secondary:#484c59;--muted:#666a79;--accent:#b52b7c;--accent-hover:#9f226b;--accent-active:#8a1d5d;\
--accent-soft:#fbeaf4;--on-accent:#fff;--error:#c2372b;--error-soft:#fbe7e5}}\
@media (pointer:coarse){:root{--h:44px;--h-lg:48px}}\
*,::before,::after{box-sizing:border-box}\
html{-webkit-text-size-adjust:100%;text-size-adjust:100%}\
body{display:grid;place-items:center;min-height:100vh;min-height:100dvh;margin:0;padding:32px 16px;\
background:var(--bg);color:var(--text);font:400 14px/20px ui-sans-serif,-apple-system,BlinkMacSystemFont,\
\"Segoe UI\",Roboto,\"Noto Sans\",\"Liberation Sans\",Arial,system-ui,sans-serif;-webkit-font-smoothing:antialiased}\
main{display:grid;justify-items:center;gap:24px;width:min(440px,100%)}\
.card{display:grid;gap:16px;width:100%;padding:32px;border:1px solid var(--subtle);border-radius:12px;\
background:var(--surface)}\
.mark{width:32px;height:32px;fill:var(--accent)}\
h1{margin:0;font-size:20px;line-height:28px;font-weight:600;letter-spacing:-.022em;overflow-wrap:anywhere}\
p{margin:0;font-size:13px}\
.muted,.code,.foot{color:var(--muted)}\
.code{font-weight:500;font-variant-numeric:tabular-nums}\
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,\"Liberation Mono\",monospace;font-size:.93em;overflow-wrap:anywhere}\
form{display:grid;gap:16px;margin:8px 0 0}\
.field{display:grid;gap:6px}\
label{color:var(--secondary);font-size:13px;font-weight:500}\
input{width:100%;height:var(--h);padding:0 10px;border:1px solid var(--control);border-radius:6px;\
background:var(--sunken);color:var(--text);font:inherit;font-size:14px}\
input:hover{border-color:var(--muted)}\
input:focus-visible{outline:0;border-color:var(--accent);box-shadow:0 0 0 3px var(--accent-soft)}\
input[aria-invalid=true]{border-color:var(--error)}\
input[aria-invalid=true]:focus-visible{box-shadow:0 0 0 3px var(--error-soft)}\
.error{display:flex;align-items:flex-start;gap:6px;color:var(--error);font-size:12px;line-height:16px}\
.error svg{flex:none;width:14px;height:14px;margin-top:1px;fill:none;stroke:currentColor;stroke-width:1.75;\
stroke-linecap:round;stroke-linejoin:round}\
.btn{display:flex;align-items:center;justify-content:center;width:100%;min-height:var(--h-lg);\
padding:4px 16px;border:1px solid var(--border);border-radius:6px;background:var(--raised);color:var(--text);\
font:inherit;font-size:14px;font-weight:500;line-height:1.25;text-align:center;text-decoration:none;\
overflow-wrap:anywhere;cursor:pointer}\
.btn:hover{background:var(--hover)}\
.primary{border-color:var(--accent);background:var(--accent);color:var(--on-accent);font-weight:600}\
.primary:hover{border-color:var(--accent-hover);background:var(--accent-hover)}\
.primary:active{border-color:var(--accent-active);background:var(--accent-active)}\
:focus-visible{outline:2px solid var(--accent);outline-offset:2px}\
.foot{font-size:12px}";

/// No scripts, no external resources, and only the stylesheet above.
fn content_security_policy() -> HeaderValue {
    static POLICY: OnceLock<HeaderValue> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            let hash = base64(&Sha256::digest(STYLE.as_bytes()));
            HeaderValue::from_str(&format!(
                "default-src 'none'; style-src 'sha256-{hash}'; base-uri 'none'; frame-ancestors 'none'"
            ))
            .expect("the policy is visible ASCII")
        })
        .clone()
}

fn html(status: StatusCode, copy: &Strings, title: &str, content: &str, footer: bool) -> Response {
    let lang = copy.lang;
    let footer = if footer {
        format!("<p class=\"foot\">{}</p>", copy.footer)
    } else {
        String::new()
    };
    (
        status,
        [
            (header::CONTENT_SECURITY_POLICY, content_security_policy()),
            (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        ],
        Html(format!(
            "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <meta name=\"robots\" content=\"noindex,nofollow\"><title>{title}</title>\
             <style>{STYLE}</style></head><body><main><section class=\"card\">{MARK}{content}</section>\
             {footer}</main></body></html>"
        )),
    )
        .into_response()
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| {
                value | (u32::from(*byte) << (16 - 8 * index))
            });
        for position in 0..4 {
            if position <= chunk.len() {
                let index = (value >> (18 - 6 * position)) & 63;
                encoded.push(char::from(ALPHABET[index as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(
            base64(&Sha256::digest(b"")),
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }

    #[test]
    fn the_policy_allows_exactly_the_embedded_stylesheet() {
        let policy = content_security_policy();
        let policy = policy.to_str().unwrap();
        let hash = base64(&Sha256::digest(STYLE.as_bytes()));
        assert!(policy.contains(&format!("style-src 'sha256-{hash}'")));
        assert!(!policy.contains("unsafe"));
        assert!(!policy.contains("script-src"));
        assert!(!STYLE.contains("</"));
    }

    async fn body(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), 16_384).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn pages_escape_every_interpolated_value() {
        let action = Action {
            link: Challenge {
                id: 1,
                revision: 1,
                password_required: true,
            },
            origin: "https://go.example\"><i>",
            path_and_query: "/a'<b>?q=&<c>",
        };
        let page = body(action.page(Some("en"), true)).await;
        assert!(page.contains(
            "action=\"https://go.example&quot;&gt;&lt;i&gt;/a&#39;&lt;b&gt;?q=&amp;&lt;c&gt;\""
        ));
        assert!(
            page.contains(
                "<span class=\"mono\">go.example&quot;&gt;&lt;i&gt;/a&#39;&lt;b&gt;</span>"
            )
        );
        assert!(!page.contains("<i>") && !page.contains("<b>") && !page.contains("<c>"));

        let home = Home {
            href: "https://go.example/\"><i>".into(),
            label: "<b>&".into(),
        };
        let missing = notice(
            &Method::GET,
            StatusCode::NOT_FOUND,
            Notice::Missing { home: Some(home) },
            None,
        );
        let missing = body(missing).await;
        assert!(
            missing.contains(
                "href=\"https://go.example/&quot;&gt;&lt;i&gt;\">Go to &lt;b&gt;&amp;</a>"
            )
        );
        assert_eq!(missing.matches("<style>").count(), 1);
    }

    #[tokio::test]
    async fn the_admin_address_notice_names_nothing_about_the_installation() {
        for (language, title) in [
            (None, ENGLISH.admin_title),
            (Some("de-AT"), GERMAN.admin_title),
        ] {
            let page = notice(
                &Method::GET,
                StatusCode::NOT_FOUND,
                Notice::AdminAddress,
                language,
            );
            assert_eq!(page.status(), StatusCode::NOT_FOUND);
            let page = body(page).await;
            assert!(page.contains(&format!("<h1>{title}</h1>")));
            assert!(!page.contains("ShortCircuit") && !page.contains("class=\"foot\""));
            assert!(!page.contains("<script") && !page.contains("href="));
        }
        let head = notice(
            &Method::HEAD,
            StatusCode::NOT_FOUND,
            Notice::AdminAddress,
            None,
        );
        assert!(body(head).await.is_empty());
    }

    #[test]
    fn the_first_preferred_language_selects_the_copy() {
        for (header, lang) in [
            (None, "en"),
            (Some("de"), "de"),
            (Some("DE-ch, en;q=0.8"), "de"),
            (Some("de;q=0.9"), "de"),
            (Some("en-GB,de;q=0.9"), "en"),
            (Some("fr"), "en"),
            (Some("deu"), "en"),
        ] {
            assert_eq!(Strings::for_language(header).lang, lang, "{header:?}");
        }
    }

    #[test]
    fn escape_covers_markup_and_attribute_delimiters() {
        assert_eq!(
            escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }
}
