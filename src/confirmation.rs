use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
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
        let action = format!("{}{}", self.origin, self.path_and_query)
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let german = language
            .and_then(|value| value.split(['-', ',', ';']).next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("de"));
        let (lang, title, password, button, error) = if german {
            (
                "de",
                "Linkzugriff bestätigen",
                "Passwort",
                "Weiter",
                "Das Passwort ist falsch.",
            )
        } else {
            (
                "en",
                "Confirm link access",
                "Password",
                "Continue",
                "The password is incorrect.",
            )
        };
        let password = if self.link.password_required {
            format!(
                "<label>{password}<input type=\"password\" name=\"password\" autocomplete=\"current-password\" required></label>"
            )
        } else {
            String::new()
        };
        let (status, feedback) = if wrong_password {
            (
                StatusCode::FORBIDDEN,
                format!("<p role=\"alert\">{error}</p>"),
            )
        } else {
            (StatusCode::OK, String::new())
        };
        (
            status,
            [
                (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'"),
                (header::REFERRER_POLICY, "strict-origin"),
                (header::X_FRAME_OPTIONS, "DENY"),
            ],
            Html(format!(
                "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
                 <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
                 <meta name=\"robots\" content=\"noindex,nofollow\"><title>{title}</title>\
                 <style>html{{color-scheme:light dark;font:1rem system-ui}}\
                 body{{max-width:26rem;margin:12vh auto;padding:1.5rem}}\
                 h1{{font-size:1.5rem}}label,input,button{{display:block;font:inherit}}\
                 input,button{{margin-top:.5rem;padding:.45rem;max-width:100%}}\
                 button{{margin-top:1rem}}</style></head><body><main><h1>{title}</h1>{feedback}\
                 <form method=\"post\" action=\"{action}\"><input type=\"hidden\" name=\"challenge\" value=\"{token}\">\
                 {password}<button type=\"submit\">{button}</button></form></main></body></html>"
            )),
        )
            .into_response()
    }
}
