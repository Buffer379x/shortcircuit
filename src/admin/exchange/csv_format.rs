use std::borrow::Cow;

use super::{BODY_LIMIT, EXCHANGE_VERSION, LinkMetadata, MAX_ITEMS};

const HEADER: [&str; 8] = [
    "version",
    "host",
    "slug",
    "target",
    "title",
    "tags",
    "enabled",
    "analytics_enabled",
];

#[derive(Debug)]
pub(super) struct CsvError;

pub(super) fn parse(input: &[u8]) -> Result<Vec<LinkMetadata>, CsvError> {
    if input.len() > BODY_LIMIT {
        return Err(CsvError);
    }
    let mut reader = csv::Reader::from_reader(input);
    if !reader.headers().map_err(|_| CsvError)?.iter().eq(HEADER) {
        return Err(CsvError);
    }

    let version = EXCHANGE_VERSION.to_string();
    let mut items = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|_| CsvError)?;
        if items.len() == MAX_ITEMS
            || record.len() != HEADER.len()
            || record.get(0) != Some(version.as_str())
        {
            return Err(CsvError);
        }
        items.push(LinkMetadata {
            host: unprotect(&record[1]).to_owned(),
            slug: unprotect(&record[2]).to_owned(),
            target: unprotect(&record[3]).to_owned(),
            title: unprotect(&record[4]).to_owned(),
            tags: serde_json::from_str(unprotect(&record[5])).map_err(|_| CsvError)?,
            enabled: parse_bool(&record[6])?,
            analytics_enabled: parse_bool(&record[7])?,
        });
    }
    if items.is_empty() {
        return Err(CsvError);
    }
    Ok(items)
}

pub(super) fn page(rows: Vec<(i64, LinkMetadata)>) -> Result<(Vec<u8>, Option<i64>), CsvError> {
    let mut header = HEADER.join(",").into_bytes();
    header.push(b'\n');
    let mut included_bytes = header.len();
    let mut writer = csv::WriterBuilder::new()
        .quote_style(csv::QuoteStyle::Always)
        .from_writer(header);
    let version = EXCHANGE_VERSION.to_string();
    let mut last_id = None;
    let mut cursor = None;
    for (count, (id, item)) in rows.into_iter().enumerate() {
        if count == MAX_ITEMS {
            cursor = last_id;
            break;
        }
        let tags = serde_json::to_string(&item.tags).map_err(|_| CsvError)?;
        writer
            .write_record([
                version.as_str(),
                protect(&item.host).as_ref(),
                protect(&item.slug).as_ref(),
                protect(&item.target).as_ref(),
                protect(&item.title).as_ref(),
                protect(&tags).as_ref(),
                if item.enabled { "true" } else { "false" },
                if item.analytics_enabled {
                    "true"
                } else {
                    "false"
                },
            ])
            .map_err(|_| CsvError)?;
        writer.flush().map_err(|_| CsvError)?;

        let encoded_bytes = writer.get_ref().len();
        if encoded_bytes > BODY_LIMIT {
            cursor = Some(last_id.ok_or(CsvError)?);
            break;
        }
        included_bytes = encoded_bytes;
        last_id = Some(id);
    }
    let mut output = writer.into_inner().map_err(|_| CsvError)?;
    output.truncate(included_bytes);
    Ok((output, cursor))
}

fn parse_bool(value: &str) -> Result<bool, CsvError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CsvError),
    }
}

fn needs_protection(value: &str) -> bool {
    let trimmed = value.trim_start();
    let leading = &value[..value.len() - trimmed.len()];
    leading
        .bytes()
        .any(|byte| matches!(byte, b'\t' | b'\r' | b'\n'))
        || trimmed.starts_with(['\'', '=', '+', '-', '@', '＝', '＋', '－', '＠'])
}

fn protect(value: &str) -> Cow<'_, str> {
    if needs_protection(value) {
        Cow::Owned(format!("'{value}"))
    } else {
        Cow::Borrowed(value)
    }
}

fn unprotect(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .filter(|text| needs_protection(text))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(title: &str) -> LinkMetadata {
        LinkMetadata {
            host: "GO.example.".into(),
            slug: "-Team/Docs".into(),
            target: "https://files.example/A%2fB?x=A+B&x=C#Keep".into(),
            title: title.into(),
            tags: vec!["Group \"A\"".into(), "@friends".into()],
            enabled: false,
            analytics_enabled: true,
        }
    }

    #[test]
    fn native_roundtrip_preserves_exact_link_metadata() {
        let (bytes, after) = page(vec![(11, link("Reports, \"May\""))]).unwrap();
        assert_eq!(after, None);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.starts_with("version,host,slug,target,title,tags,enabled,analytics_enabled\n")
        );
        assert!(text.contains("\"'-Team/Docs\""));
        assert!(text.contains("\"Reports, \"\"May\"\"\""));
        assert!(text.contains("https://files.example/A%2fB?x=A+B&x=C#Keep"));
        assert!(parse(&bytes).unwrap() == [link("Reports, \"May\"")]);
    }

    #[test]
    fn formula_and_apostrophe_hardening_is_reversible() {
        for title in [
            "=1+1",
            "  +value",
            " -value",
            "@value",
            "＝1",
            "＋1",
            "－1",
            "＠value",
            "'existing apostrophe",
            "  'existing apostrophe",
            "\ttext",
            "  \ttabs",
            "\rtext",
            "\ntext",
            "\u{2003}=after Unicode whitespace",
        ] {
            let (bytes, _) = page(vec![(1, link(title))]).unwrap();
            let raw = csv::Reader::from_reader(bytes.as_slice())
                .records()
                .next()
                .unwrap()
                .unwrap();
            let protected = format!("'{title}");
            assert_eq!(raw.get(4), Some(protected.as_str()), "{title:?}");
            assert_eq!(parse(&bytes).unwrap()[0].title, title);
        }
        let (bytes, _) = page(vec![(1, link("  ordinary title"))]).unwrap();
        assert!(
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains("\"  ordinary title\"")
        );
        assert_eq!(parse(&bytes).unwrap()[0].title, "  ordinary title");
    }

    #[test]
    fn parse_rejects_invalid_header_version_types_and_utf8() {
        let header = "version,host,slug,target,title,tags,enabled,analytics_enabled";
        let row = "1,go.example,docs,https://files.example/,Title,[],true,false\n";
        let input = format!("{header}\n{row}");
        assert!(parse(input.as_bytes()).is_ok());
        for invalid in [
            format!("{}\n{row}", header.replace("title,tags", "tags,title")),
            format!("{header}\n{}", row.replacen("1,", "01,", 1)),
            format!("{header}\n{}", row.replacen("1,", "2,", 1)),
            format!("{header}\n{}", row.replace(",true,false", ",TRUE,false")),
            format!("{header}\n{}", row.replace("[]", "null")),
            format!("{header}\n{}", row.replace(",Title", "")),
            format!("{header}\n"),
        ] {
            assert!(parse(invalid.as_bytes()).is_err(), "{invalid:?}");
        }
        let mut invalid_encoding = input.into_bytes();
        let title = invalid_encoding
            .iter()
            .position(|byte| *byte == b'T')
            .unwrap();
        invalid_encoding[title] = 0xff;
        assert!(parse(&invalid_encoding).is_err());
    }

    #[test]
    fn byte_cap_counts_quotes_and_stops_before_the_unsent_row() {
        let generate = |first| {
            (first..=12)
                .map(|id| {
                    let mut item = link("Quoted, \"notes\"");
                    item.slug = format!("link-{id}");
                    item.target = format!("https://files.example/?token={}", "q".repeat(3900));
                    (id, item)
                })
                .collect()
        };
        let (bytes, after) = page(generate(1)).unwrap();
        assert!(bytes.len() <= BODY_LIMIT);
        assert_eq!(after, Some(8));
        let items = parse(&bytes).unwrap();
        assert_eq!(items.len(), 8);
        assert_eq!(items[7].slug, "link-8");
        let (rest, next) = page(generate(9)).unwrap();
        assert_eq!(next, None);
        let rest = parse(&rest).unwrap();
        assert_eq!(rest.len(), 4);
        assert_eq!(rest[0].slug, "link-9");
        assert!(parse(&vec![b' '; BODY_LIMIT + 1]).is_err());
    }

    #[test]
    fn row_cap_is_bounded_on_export_and_import() {
        let rows = (1..=101).map(|id| (id, link(""))).collect();
        let (bytes, after) = page(rows).unwrap();
        assert_eq!(after, Some(100));
        assert_eq!(parse(&bytes).unwrap().len(), 100);
        let mut overflow = bytes;
        overflow.extend_from_slice(b"1,go.example,extra,https://files.example/,,[],false,false\n");
        assert!(overflow.len() < BODY_LIMIT);
        assert!(parse(&overflow).is_err());
    }
}
