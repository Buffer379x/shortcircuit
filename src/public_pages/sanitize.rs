//! Allowlist sanitizer for custom public pages.
//!
//! The sanitizer tokenizes the submitted markup and writes a new document from what it kept:
//! allowlisted elements with allowlisted attributes, escaped text, escaped attribute values, and
//! checked style sheets. It never copies input markup through, so a browser parses only markup
//! that this module wrote. A parser difference between this tokenizer and a browser can drop or
//! keep text, but it cannot produce an element or attribute outside the allowlist. The Content
//! Security Policy of the public pages blocks scripts and network loads independently.

use std::collections::BTreeSet;

use super::{ImageKind, decode_base64};

/// The largest accepted source, in bytes.
pub(crate) const MAX_SOURCE_BYTES: usize = 64 * 1024;
/// The largest sanitized result, in bytes. Escaping can grow the source slightly.
pub(crate) const MAX_SANITIZED_BYTES: usize = 128 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Host,
    Lang,
    RootLink,
    Form,
}

impl Slot {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "host" => Some(Self::Host),
            "lang" => Some(Self::Lang),
            "root_link" => Some(Self::RootLink),
            "form" => Some(Self::Form),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Lang => "lang",
            Self::RootLink => "root_link",
            Self::Form => "form",
        }
    }

    /// `{{host}}` and `{{lang}}` are plain values. The others insert markup and belong in text.
    fn inline(self) -> bool {
        matches!(self, Self::Host | Self::Lang)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Markup(String),
    Slot(Slot),
}

#[derive(Debug, Default)]
pub(crate) struct Sanitized {
    /// Canonical markup with placeholders written as `{{name}}`. This is the stored form.
    pub(crate) source: String,
    /// The first `<title>`, as escaped text and inline placeholders.
    pub(crate) title: Option<Vec<Part>>,
    /// Checked style sheets in document order.
    pub(crate) styles: Vec<String>,
    /// Body markup and placeholders.
    pub(crate) body: Vec<Part>,
    /// What the sanitizer removed, as stable category names.
    pub(crate) removed: BTreeSet<&'static str>,
    /// How often `{{form}}` occurs.
    pub(crate) forms: usize,
}

impl Sanitized {
    fn markup(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.source.push_str(text);
        match self.body.last_mut() {
            Some(Part::Markup(last)) => last.push_str(text),
            _ => self.body.push(Part::Markup(text.to_owned())),
        }
    }

    fn slot(&mut self, slot: Slot) {
        self.source.push_str("{{");
        self.source.push_str(slot.name());
        self.source.push_str("}}");
        if slot == Slot::Form {
            self.forms += 1;
        }
        self.body.push(Part::Slot(slot));
    }
}

const ALLOWED: &[&str] = &[
    "a",
    "abbr",
    "article",
    "aside",
    "b",
    "blockquote",
    "br",
    "caption",
    "code",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "main",
    "mark",
    "nav",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "section",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
    "wbr",
];
const VOID: &[&str] = &["br", "hr", "img", "wbr"];
/// Elements whose content the browser reads as raw text. They are dropped with their content,
/// except `style` and `title`, which the sanitizer reads itself.
const RAW_TEXT: &[(&str, &str)] = &[
    ("script", "script"),
    ("textarea", "form"),
    ("xmp", "element"),
    ("iframe", "frame"),
    ("noembed", "plugin"),
    ("noframes", "frame"),
    ("noscript", "script"),
];
/// Elements dropped together with everything they contain.
const DROP_SUBTREE: &[(&str, &str)] = &[
    ("svg", "element"),
    ("math", "element"),
    ("template", "element"),
    ("object", "plugin"),
    ("applet", "plugin"),
    ("form", "form"),
    ("select", "form"),
    ("button", "form"),
    ("frameset", "frame"),
];
/// Void or content-free elements that are dropped.
const DROP_TAG: &[(&str, &str)] = &[
    ("embed", "plugin"),
    ("param", "plugin"),
    ("frame", "frame"),
    ("input", "form"),
    ("option", "form"),
    ("meta", "meta"),
    ("link", "meta"),
    ("base", "meta"),
    ("source", "element"),
    ("track", "element"),
    ("area", "element"),
];
/// Wrappers of a full document. The tag is dropped and the content kept, without a report.
const UNWRAP_QUIETLY: &[&str] = &["html", "head", "body"];
const GLOBAL_ATTRIBUTES: &[&str] = &[
    "class",
    "id",
    "title",
    "lang",
    "dir",
    "role",
    "aria-label",
    "aria-labelledby",
    "aria-describedby",
    "aria-hidden",
];

/// Why a source cannot be saved.
pub(crate) type Refusal = &'static str;

pub(crate) fn sanitize(input: &str) -> Result<Sanitized, Refusal> {
    if input.len() > MAX_SOURCE_BYTES {
        return Err("page_too_large");
    }
    let mut out = Sanitized::default();
    let mut open: Vec<&'static str> = Vec::new();
    // A dropped subtree: its element name and nesting depth.
    let mut skip: Option<(String, usize)> = None;
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            let end = input[index..]
                .find('<')
                .map_or(input.len(), |at| index + at);
            if skip.is_none() {
                text(&mut out, &input[index..end])?;
            }
            index = end;
            continue;
        }
        let rest = &input[index..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            index = comment
                .find("-->")
                .map_or(input.len(), |at| index + 4 + at + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            index = rest.find('>').map_or(input.len(), |at| index + at + 1);
            continue;
        }
        let closing = rest.starts_with("</");
        let name_start = if closing { 2 } else { 1 };
        if !rest
            .as_bytes()
            .get(name_start)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            if closing {
                // `</` followed by a non-letter is a bogus comment up to the next `>`.
                index = rest.find('>').map_or(input.len(), |at| index + at + 1);
            } else {
                if skip.is_none() {
                    out.markup("&lt;");
                }
                index += 1;
            }
            continue;
        }
        let Some(tag) = parse_tag(rest, name_start) else {
            // A tag without its closing `>` ends the document, as in a browser.
            break;
        };
        index += tag.length;
        if closing {
            if let Some((name, depth)) = &mut skip {
                if *name == tag.name {
                    *depth -= 1;
                    if *depth == 0 {
                        skip = None;
                    }
                }
                continue;
            }
            if let Some(position) = open.iter().rposition(|name| *name == tag.name) {
                for name in open.drain(position..).rev() {
                    out.markup(&format!("</{name}>"));
                }
            }
            continue;
        }
        let name = tag.name.as_str();
        // Raw text is consumed even inside a dropped subtree, so its content cannot reopen
        // markup there.
        if matches!(name, "style" | "title" | "plaintext")
            || RAW_TEXT.iter().any(|(raw, _)| *raw == name)
        {
            if name == "plaintext" {
                out.removed.insert("element");
                break;
            }
            let (content, consumed) = raw_text(&input[index..], name);
            index += consumed;
            if skip.is_some() {
                continue;
            }
            match name {
                "style" => {
                    let css = stylesheet(content)?;
                    out.source.push_str("<style>");
                    out.source.push_str(&css);
                    out.source.push_str("</style>");
                    out.styles.push(css);
                }
                "title" => {
                    if out.title.is_none() {
                        let mut title = Sanitized::default();
                        text_inline(&mut title, content)?;
                        out.source.push_str("<title>");
                        out.source.push_str(&title.source);
                        out.source.push_str("</title>");
                        out.title = Some(title.body);
                    }
                }
                _ => {
                    if let Some((_, category)) = RAW_TEXT.iter().find(|(raw, _)| *raw == name) {
                        out.removed.insert(category);
                    }
                }
            }
            continue;
        }
        if let Some((current, depth)) = &mut skip {
            if *current == name && !tag.self_closing {
                *depth += 1;
            }
            continue;
        }
        if let Some((_, category)) = DROP_SUBTREE.iter().find(|(drop, _)| *drop == name) {
            out.removed.insert(category);
            if !(tag.self_closing && matches!(name, "svg" | "math")) {
                skip = Some((name.to_owned(), 1));
            }
            continue;
        }
        if let Some((_, category)) = DROP_TAG.iter().find(|(drop, _)| *drop == name) {
            out.removed.insert(category);
            continue;
        }
        let Some(allowed) = ALLOWED.iter().copied().find(|allowed| *allowed == name) else {
            if !UNWRAP_QUIETLY.contains(&name) {
                out.removed.insert("element");
            }
            continue;
        };
        if open.len() >= MAX_DEPTH {
            out.removed.insert("element");
            continue;
        }
        let attributes = attributes(&mut out, allowed, &tag.attributes)?;
        out.markup(&format!("<{allowed}"));
        for (name, value) in attributes {
            out.markup(&format!(" {name}=\""));
            for part in value {
                match part {
                    Part::Markup(text) => out.markup(&text),
                    Part::Slot(slot) => out.slot(slot),
                }
            }
            out.markup("\"");
        }
        out.markup(">");
        if !VOID.contains(&allowed) {
            open.push(allowed);
        }
    }
    for name in open.into_iter().rev() {
        out.markup(&format!("</{name}>"));
    }
    if out.source.len() > MAX_SANITIZED_BYTES {
        return Err("page_too_large");
    }
    Ok(out)
}

struct Tag {
    name: String,
    attributes: Vec<(String, String)>,
    self_closing: bool,
    length: usize,
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// Reads a start or end tag beginning at `<`. Returns `None` when the input ends inside it.
fn parse_tag(input: &str, name_start: usize) -> Option<Tag> {
    let bytes = input.as_bytes();
    let mut index = name_start;
    while index < bytes.len() && !is_space(bytes[index]) && !matches!(bytes[index], b'/' | b'>') {
        index += 1;
    }
    let name = input[name_start..index].to_ascii_lowercase();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let mut self_closing = false;
    loop {
        while index < bytes.len() && (is_space(bytes[index]) || bytes[index] == b'/') {
            self_closing = bytes[index] == b'/';
            index += 1;
        }
        match bytes.get(index) {
            None => return None,
            Some(b'>') => {
                return Some(Tag {
                    name,
                    attributes,
                    self_closing,
                    length: index + 1,
                });
            }
            _ => {}
        }
        self_closing = false;
        let start = index;
        index += 1;
        while index < bytes.len()
            && !is_space(bytes[index])
            && !matches!(bytes[index], b'/' | b'>' | b'=')
        {
            index += 1;
        }
        let attribute = input[start..index].to_ascii_lowercase();
        while index < bytes.len() && is_space(bytes[index]) {
            index += 1;
        }
        let mut value = String::new();
        if bytes.get(index) == Some(&b'=') {
            index += 1;
            while index < bytes.len() && is_space(bytes[index]) {
                index += 1;
            }
            match bytes.get(index) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let end = input[index + 1..].find(char::from(quote))? + index + 1;
                    value = input[index + 1..end].to_owned();
                    index = end + 1;
                }
                _ => {
                    let start = index;
                    while index < bytes.len() && !is_space(bytes[index]) && bytes[index] != b'>' {
                        index += 1;
                    }
                    value = input[start..index].to_owned();
                }
            }
        }
        // The first occurrence of an attribute wins, as in a browser.
        if !attributes.iter().any(|(name, _)| *name == attribute) {
            attributes.push((attribute, value));
        }
    }
}

/// Splits raw text content from the input after a start tag. Returns the content and the
/// number of bytes consumed, including the end tag.
fn raw_text<'a>(input: &'a str, name: &str) -> (&'a str, usize) {
    let lower = input.to_ascii_lowercase();
    let needle = format!("</{name}");
    let mut from = 0;
    while let Some(at) = lower[from..].find(&needle) {
        let start = from + at;
        let after = start + needle.len();
        match lower.as_bytes().get(after) {
            None => return (&input[..start], input.len()),
            Some(&byte) if is_space(byte) || byte == b'/' || byte == b'>' => {
                let end = input[after..]
                    .find('>')
                    .map_or(input.len(), |gt| after + gt + 1);
                return (&input[..start], end);
            }
            _ => from = after,
        }
    }
    (input, input.len())
}

/// Writes body text, with markup placeholders allowed.
fn text(out: &mut Sanitized, input: &str) -> Result<(), Refusal> {
    let mut rest = input;
    while let Some(at) = rest.find("{{") {
        out.markup(&escape_text(&rest[..at]));
        let after = &rest[at + 2..];
        let end = after.find("}}").ok_or("invalid_placeholder")?;
        let slot = Slot::parse(&after[..end]).ok_or("invalid_placeholder")?;
        out.slot(slot);
        rest = &after[end + 2..];
    }
    out.markup(&escape_text(rest));
    Ok(())
}

/// Writes title text, where only inline placeholders are allowed.
fn text_inline(out: &mut Sanitized, input: &str) -> Result<(), Refusal> {
    let mut check = input;
    while let Some(at) = check.find("{{") {
        let after = &check[at + 2..];
        let end = after.find("}}").ok_or("invalid_placeholder")?;
        if !Slot::parse(&after[..end]).is_some_and(Slot::inline) {
            return Err("invalid_placeholder");
        }
        check = &after[end + 2..];
    }
    text(out, input)
}

/// Escapes text. Character references stay intact, because a reference in text always decodes
/// to text; every other `&`, `<`, and `>` is escaped. NUL characters are dropped.
fn escape_text(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        match character {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\0' => {}
            '&' => {
                if let Some(length) = reference_length(&input[index..]) {
                    escaped.push_str(&input[index..index + length]);
                    for _ in 1..length {
                        chars.next();
                    }
                } else {
                    escaped.push_str("&amp;");
                }
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

/// The length of a well-formed character reference at the start of `input`, if it is one.
fn reference_length(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let body = &bytes[1..];
    let length = if body.first() == Some(&b'#') {
        let (digits, hex) = match body.get(1) {
            Some(b'x' | b'X') => (&body[2..], true),
            _ => (&body[1..], false),
        };
        let count = digits
            .iter()
            .take_while(|byte| {
                if hex {
                    byte.is_ascii_hexdigit()
                } else {
                    byte.is_ascii_digit()
                }
            })
            .count();
        if count == 0 || count > 7 {
            return None;
        }
        1 + usize::from(hex) + count
    } else {
        let count = body
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric())
            .count();
        if count == 0 || count > 32 || !body[0].is_ascii_alphabetic() {
            return None;
        }
        count
    };
    (body.get(length) == Some(&b';')).then_some(length + 2)
}

/// Decodes the references this sanitizer understands. Any other reference stays literal text,
/// and the value is escaped again on output, so the browser reads exactly the decoded value.
fn decode_references(input: &str) -> String {
    let mut decoded = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('&') {
        decoded.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(length) = reference_length(rest) else {
            decoded.push('&');
            rest = &rest[1..];
            continue;
        };
        let body = &rest[1..length - 1];
        let character = if let Some(number) = body.strip_prefix('#') {
            let value = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => number.parse().ok(),
            };
            Some(
                value
                    .and_then(char::from_u32)
                    .filter(|c| *c != '\0')
                    .unwrap_or('\u{fffd}'),
            )
        } else {
            match body {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "colon" => Some(':'),
                "sol" => Some('/'),
                "tab" => Some('\t'),
                "newline" => Some('\n'),
                _ => None,
            }
        };
        match character {
            Some(character) => decoded.push(character),
            None => decoded.push_str(&rest[..length]),
        }
        rest = &rest[length..];
    }
    decoded.push_str(rest);
    decoded.replace('\0', "")
}

fn escape_attribute(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for character in input.chars() {
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

/// Splits a decoded attribute value into escaped text and inline placeholders.
fn attribute_parts(value: &str) -> Result<Vec<Part>, Refusal> {
    let mut parts = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.find("{{") {
        if at > 0 {
            parts.push(Part::Markup(escape_attribute(&rest[..at])));
        }
        let after = &rest[at + 2..];
        let end = after.find("}}").ok_or("invalid_placeholder")?;
        let slot = Slot::parse(&after[..end])
            .filter(|slot| slot.inline())
            .ok_or("invalid_placeholder")?;
        parts.push(Part::Slot(slot));
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        parts.push(Part::Markup(escape_attribute(rest)));
    }
    Ok(parts)
}

fn attributes(
    out: &mut Sanitized,
    element: &'static str,
    input: &[(String, String)],
) -> Result<Vec<(&'static str, Vec<Part>)>, Refusal> {
    let mut kept = Vec::new();
    for (name, raw) in input {
        let value = decode_references(raw);
        let allowed = GLOBAL_ATTRIBUTES
            .iter()
            .chain(match element {
                "a" => &["href", "rel", "hreflang"][..],
                "img" => &["src", "alt", "width", "height"][..],
                "td" | "th" => &["colspan", "rowspan"][..],
                "ol" => &["start", "reversed"][..],
                "details" => &["open"][..],
                _ => &[][..],
            })
            .copied()
            .find(|allowed| *allowed == name);
        let Some(allowed) = allowed else {
            out.removed.insert(if name.starts_with("on") {
                "event_handler"
            } else if name == "style" {
                "style_attribute"
            } else if matches!(
                name.as_str(),
                "src"
                    | "srcset"
                    | "href"
                    | "action"
                    | "formaction"
                    | "xlink:href"
                    | "poster"
                    | "background"
                    | "data"
                    | "ping"
                    | "cite"
                    | "longdesc"
            ) {
                "unsafe_url"
            } else {
                "attribute"
            });
            continue;
        };
        let value = match allowed {
            "href" => match safe_link(&value) {
                Some(link) => link,
                None => {
                    out.removed.insert("unsafe_url");
                    continue;
                }
            },
            "src" => match image_source(&value) {
                Some(source) => source,
                None => {
                    out.removed.insert("unsafe_url");
                    continue;
                }
            },
            "width" | "height" | "colspan" | "rowspan" | "start" => {
                if value.is_empty()
                    || value.len() > 4
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    out.removed.insert("attribute");
                    continue;
                }
                value
            }
            "dir" if !matches!(value.as_str(), "ltr" | "rtl" | "auto") => {
                out.removed.insert("attribute");
                continue;
            }
            "lang" | "hreflang"
                if value != "{{lang}}"
                    && (value.is_empty()
                        || value.len() > 35
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')) =>
            {
                out.removed.insert("attribute");
                continue;
            }
            _ => value,
        };
        kept.push((allowed, attribute_parts(&value)?));
    }
    Ok(kept)
}

/// Accepts HTTP and HTTPS links and links relative to the page. Returns the value a browser
/// uses after it removes tabs and line breaks.
fn safe_link(value: &str) -> Option<String> {
    let cleaned: String = value
        .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_control())
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    if cleaned.chars().any(|c| c.is_control()) {
        return None;
    }
    match cleaned.find([':', '/', '?', '#']) {
        Some(at) if cleaned.as_bytes()[at] == b':' => {
            let scheme = cleaned[..at].to_ascii_lowercase();
            matches!(scheme.as_str(), "http" | "https").then_some(cleaned)
        }
        _ => Some(cleaned),
    }
}

/// Accepts only an inline PNG, JPEG, or WebP image whose bytes match its declared type.
fn image_source(value: &str) -> Option<String> {
    let compact: String = value.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let (kind, data) = data_image(&compact)?;
    let bytes = decode_base64(data)?;
    (ImageKind::sniff(&bytes) == Some(kind)).then(|| format!("data:{};base64,{data}", kind.mime()))
}

fn data_image(value: &str) -> Option<(ImageKind, &str)> {
    let lower = value.get(..24)?.to_ascii_lowercase();
    for kind in [ImageKind::Png, ImageKind::Jpeg, ImageKind::Webp] {
        let prefix = format!("data:{};base64,", kind.mime());
        if lower.starts_with(&prefix) {
            return Some((kind, &value[prefix.len()..]));
        }
    }
    None
}

/// Checks a style sheet. It refuses constructs that load resources or run code instead of
/// guessing a safe subset: imports, non-image or remote `url()` values, CSS escapes, and legacy
/// script hooks. Comments are removed. `<` is refused, so a style sheet can never end its element.
pub(crate) fn stylesheet(input: &str) -> Result<String, Refusal> {
    let mut css = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find("/*") {
        css.push_str(&rest[..at]);
        css.push(' ');
        let end = rest[at + 2..].find("*/").ok_or("unsafe_css")?;
        rest = &rest[at + 2 + end + 2..];
    }
    css.push_str(rest);
    if css.contains(['<', '\\', '\0']) {
        return Err("unsafe_css");
    }
    let lower = css.to_ascii_lowercase();
    for forbidden in [
        "@import",
        "@namespace",
        "expression(",
        "javascript:",
        "vbscript:",
        "-moz-binding",
        "behavior",
        "image-set(",
        "image(",
        "src(",
        "element(",
        "cross-fade(",
        "paint(",
    ] {
        if lower.contains(forbidden) {
            return Err("unsafe_css");
        }
    }
    let mut from = 0;
    while let Some(at) = lower[from..].find("url(") {
        let start = from + at + 4;
        let inner = css[start..].trim_start();
        let offset = css.len() - start - inner.len();
        let (quote, inner) = match inner.as_bytes().first() {
            Some(&quote @ (b'"' | b'\'')) => (Some(char::from(quote)), &inner[1..]),
            _ => (None, inner),
        };
        let end = match quote {
            Some(quote) => inner.find(quote).ok_or("unsafe_css")?,
            None => inner
                .find(|c: char| c == ')' || c.is_ascii_whitespace())
                .ok_or("unsafe_css")?,
        };
        let (kind, data) = data_image(&inner[..end]).ok_or("unsafe_css")?;
        let bytes = decode_base64(data).ok_or("unsafe_css")?;
        if ImageKind::sniff(&bytes) != Some(kind) {
            return Err("unsafe_css");
        }
        from = start + offset + usize::from(quote.is_some()) + end;
    }
    Ok(css)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn clean(input: &str) -> Sanitized {
        sanitize(input).unwrap()
    }

    fn rendered(parts: &[Part]) -> String {
        parts
            .iter()
            .map(|part| match part {
                Part::Markup(text) => text.clone(),
                Part::Slot(slot) => format!("{{{{{}}}}}", slot.name()),
            })
            .collect()
    }

    #[test]
    fn scripts_and_their_content_are_removed() {
        for input in [
            "<p>a</p><script>alert(1)</script><p>b</p>",
            "<p>a</p><SCRIPT type=module>alert(1)</SCRIPT ><p>b</p>",
            "<p>a</p><script>document.write('</scr'+'ipt>')</script><p>b</p>",
            "<p>a</p><noscript><img src=x onerror=alert(1)></noscript><p>b</p>",
        ] {
            let result = clean(input);
            assert!(
                !result.source.to_ascii_lowercase().contains("script"),
                "{input}"
            );
            assert!(!result.source.contains("alert"), "{input}");
            assert!(
                result.source.starts_with("<p>a</p>"),
                "{input}: {}",
                result.source
            );
            assert!(result.removed.contains("script"));
        }
    }

    #[test]
    fn event_handlers_and_style_attributes_are_removed() {
        let result =
            clean("<div onclick=\"x()\" ONMOUSEOVER='y()' style=\"color:red\" class=\"a\">t</div>");
        assert_eq!(result.source, "<div class=\"a\">t</div>");
        assert!(result.removed.contains("event_handler"));
        assert!(result.removed.contains("style_attribute"));
    }

    #[test]
    fn only_http_links_and_inline_raster_images_remain() {
        for href in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            " java\tscript:alert(1)",
            "jav&#x61;script:alert(1)",
            "jav&#97;script&colon;alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:x",
            "file:///etc/passwd",
            "\u{1}javascript:alert(1)",
        ] {
            let result = clean(&format!("<a href=\"{href}\">x</a>"));
            assert_eq!(result.source, "<a>x</a>", "{href}");
            assert!(result.removed.contains("unsafe_url"));
        }
        for href in [
            "https://example.com/a?b=1&amp;c=2",
            "http://example.com",
            "/",
            "#top",
        ] {
            let result = clean(&format!("<a href=\"{href}\">x</a>"));
            assert!(result.source.starts_with("<a href=\""), "{href}");
            assert!(result.removed.is_empty(), "{href}");
        }
        for src in [
            "https://tracker.example/pixel.png",
            "//tracker.example/p.png",
            "data:image/svg+xml;base64,PHN2Zy8+",
            "data:image/png;base64,PHN2Zy8+",
            "data:image/jpeg;base64,".to_owned().leak(),
        ] {
            let result = clean(&format!("<img src=\"{src}\" alt=\"a\">"));
            assert_eq!(result.source, "<img alt=\"a\">", "{src}");
        }
        let result = clean(&format!(
            "<img src=\"data:image/png;base64,{PNG}\" alt=\"\">"
        ));
        assert!(result.source.contains("src=\"data:image/png;base64,"));
        assert!(result.removed.is_empty());
    }

    #[test]
    fn frames_plugins_forms_and_meta_are_removed() {
        let result = clean(
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">\
             <base href=\"https://evil.example/\"><link rel=stylesheet href=\"https://evil.example/a.css\">\
             <iframe src=\"https://evil.example\">x</iframe><object data=\"x\"><p>in</p></object>\
             <embed src=\"x\"><form action=\"https://evil.example\"><input name=p><button>Go</button></form>\
             <svg><script>alert(1)</script></svg><p>kept</p>",
        );
        assert_eq!(result.source, "<p>kept</p>");
        for category in ["meta", "frame", "plugin", "form", "element"] {
            assert!(result.removed.contains(category), "{category}");
        }
    }

    #[test]
    fn markup_is_rebuilt_and_balanced() {
        let result = clean(
            "<html><head><title>T &amp; {{host}}</title></head><body><main><p>a<b>b</main>x</p>",
        );
        assert_eq!(rendered(result.title.as_ref().unwrap()), "T &amp; {{host}}");
        assert_eq!(rendered(&result.body), "<main><p>a<b>b</b></p></main>x");
        assert!(result.removed.is_empty());
        let result = clean("<p title=\"a\"b\" x=1>q</p><p title='\"><script>'>z");
        assert_eq!(
            result.source,
            "<p title=\"a\">q</p><p title=\"&quot;&gt;&lt;script&gt;\">z</p>"
        );
        assert_eq!(
            clean("a < b > c & d &copy; &#169;").source,
            "a &lt; b &gt; c &amp; d &copy; &#169;"
        );
        assert_eq!(clean("<p>a</p><div").source, "<p>a</p>");
        assert_eq!(clean("x<!-- <script>alert(1)</script> -->y").source, "xy");
    }

    #[test]
    fn placeholders_are_limited_to_their_context() {
        let result = clean(
            "<p>{{host}} {{lang}}</p>{{root_link}}{{form}}<a href=\"https://{{host}}/\">h</a>",
        );
        assert_eq!(result.forms, 1);
        assert_eq!(
            result.body,
            vec![
                Part::Markup("<p>".into()),
                Part::Slot(Slot::Host),
                Part::Markup(" ".into()),
                Part::Slot(Slot::Lang),
                Part::Markup("</p>".into()),
                Part::Slot(Slot::RootLink),
                Part::Slot(Slot::Form),
                Part::Markup("<a href=\"https://".into()),
                Part::Slot(Slot::Host),
                Part::Markup("/\">h</a>".into()),
            ]
        );
        for input in [
            "{{slug}}",
            "{{path}}",
            "{{ host }}",
            "{{host",
            "<a title=\"{{form}}\">x</a>",
            "<title>{{form}}</title>",
        ] {
            assert_eq!(
                sanitize(input).err(),
                Some("invalid_placeholder"),
                "{input}"
            );
        }
    }

    #[test]
    fn style_sheets_refuse_remote_and_escaped_constructs() {
        assert_eq!(
            stylesheet("body{color:#fff} /* c */ .a{background:url(\"data:image/png;base64,iVBORw0KGgo=\")}").unwrap(),
            "body{color:#fff}   .a{background:url(\"data:image/png;base64,iVBORw0KGgo=\")}"
        );
        for css in [
            "@import 'https://evil.example/a.css';",
            "a{background:url(https://evil.example/p.png)}",
            "a{background:URL( 'http://x' )}",
            "a{background:u\\72l(https://x)}",
            "a{background:image-set('x.png' 1x)}",
            "@font-face{src:url(https://x/font.woff)}",
            "a{width:expression(alert(1))}",
            "a{-moz-binding:url(x)}",
            "</style><script>alert(1)</script>",
            "a{background:url(data:image/svg+xml;base64,PHN2Zy8+)}",
            "a{} /* unterminated",
        ] {
            assert_eq!(stylesheet(css).err(), Some("unsafe_css"), "{css}");
        }
        let result = clean("<style>p{color:red}</style><p>x</p>");
        assert_eq!(result.styles, vec!["p{color:red}".to_owned()]);
        assert_eq!(rendered(&result.body), "<p>x</p>");
        assert_eq!(result.source, "<style>p{color:red}</style><p>x</p>");
    }

    #[test]
    fn the_size_limit_is_enforced() {
        assert_eq!(
            sanitize(&"a".repeat(MAX_SOURCE_BYTES + 1)).err(),
            Some("page_too_large")
        );
        assert!(sanitize(&"a".repeat(MAX_SOURCE_BYTES)).is_ok());
    }

    #[test]
    fn sanitizing_is_idempotent() {
        let first = clean(
            "<main class=x><h1 id=t>T</h1><p>A &amp; B<br>{{host}}</p>{{form}}\
             <a href=\"https://example.com/?a=1&b=2\" rel=noopener>l</a></main><style>h1{color:red}</style>",
        );
        let second = clean(&first.source);
        assert_eq!(first.source, second.source);
        assert_eq!(first.body, second.body);
        assert_eq!(first.styles, second.styles);
    }
}
