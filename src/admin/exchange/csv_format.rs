use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::{BODY_LIMIT, EXCHANGE_VERSION, LinkMetadata, MAX_ITEMS};

const MAX_COLUMNS: usize = 64;
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

/// A rejected import file. `row` is the 1-based data row after the header, and `field` the
/// exchange field that failed in it, when known.
#[derive(Debug, Default)]
pub(super) struct RowError {
    pub(super) row: Option<usize>,
    pub(super) field: Option<&'static str>,
}

impl From<CsvError> for RowError {
    fn from(_: CsvError) -> Self {
        Self::default()
    }
}

/// A rejected record, with the field that failed when known.
struct FieldError(Option<&'static str>);

fn field(name: &'static str) -> impl FnOnce(CsvError) -> FieldError {
    move |_| FieldError(Some(name))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportQuery {
    pub(super) preview: Option<bool>,
    host: Option<String>,
    host_column: Option<usize>,
    slug_column: Option<usize>,
    target_column: Option<usize>,
    title_column: Option<usize>,
    description_column: Option<usize>,
    tags_column: Option<usize>,
    enabled_column: Option<usize>,
    analytics_enabled_column: Option<usize>,
}

impl ImportQuery {
    pub(super) fn is_mapped(&self) -> bool {
        self.host.is_some() || self.columns().into_iter().any(|column| column.is_some())
    }

    fn columns(&self) -> [Option<usize>; 8] {
        [
            self.host_column,
            self.slug_column,
            self.target_column,
            self.title_column,
            self.description_column,
            self.tags_column,
            self.enabled_column,
            self.analytics_enabled_column,
        ]
    }
}

#[derive(Serialize)]
pub(super) struct Columns {
    columns: Vec<String>,
    native: bool,
}

pub(super) fn inspect(input: &[u8]) -> Result<Columns, RowError> {
    let (mut reader, header) = open(input)?;
    read_records(&mut reader, |_| Ok(()))?;
    Ok(Columns {
        native: is_native(&header),
        columns: header.iter().map(str::to_owned).collect(),
    })
}

pub(super) fn parse(input: &[u8]) -> Result<Vec<LinkMetadata>, RowError> {
    let (mut reader, header) = open(input)?;
    if !is_native(&header) {
        return Err(RowError::default());
    }
    let version = EXCHANGE_VERSION.to_string();
    read_records(&mut reader, |record| {
        if record.get(0) != Some(version.as_str()) {
            return Err(FieldError(Some("version")));
        }
        Ok(LinkMetadata {
            host: unprotect(&record[1]).to_owned(),
            slug: unprotect(&record[2]).to_owned(),
            target: unprotect(&record[3]).to_owned(),
            title: unprotect(&record[4]).to_owned(),
            description: record.get(8).map(unprotect).unwrap_or_default().to_owned(),
            tags: serde_json::from_str(unprotect(&record[5]))
                .map_err(|_| FieldError(Some("tags")))?,
            enabled: parse_bool(&record[6]).map_err(field("enabled"))?,
            analytics_enabled: parse_bool(&record[7]).map_err(field("analytics_enabled"))?,
        })
    })
}

pub(super) fn parse_mapped(
    input: &[u8],
    mapping: &ImportQuery,
) -> Result<Vec<LinkMetadata>, RowError> {
    let (mut reader, header) = open(input)?;
    let (Some(slug), Some(target)) = (mapping.slug_column, mapping.target_column) else {
        return Err(RowError::default());
    };
    if mapping.host.is_some() == mapping.host_column.is_some() {
        return Err(RowError::default());
    }
    let mut used = [false; MAX_COLUMNS];
    for column in mapping.columns().into_iter().flatten() {
        if column >= header.len() || used[column] {
            return Err(RowError::default());
        }
        used[column] = true;
    }
    read_records(&mut reader, |record| {
        let value =
            |column: Option<usize>| column.and_then(|index| record.get(index)).unwrap_or("");
        let host = mapping
            .host
            .as_deref()
            .unwrap_or_else(|| value(mapping.host_column));
        let description = value(mapping.description_column).replace("\r\n", "\n");
        Ok(LinkMetadata {
            host: host.to_owned(),
            slug: record[slug].to_owned(),
            target: record[target].to_owned(),
            title: value(mapping.title_column).to_owned(),
            description: description.replace('\r', "\n"),
            tags: mapped_tags(value(mapping.tags_column)).map_err(field("tags"))?,
            enabled: mapped_bool(value(mapping.enabled_column)).map_err(field("enabled"))?,
            analytics_enabled: mapped_bool(value(mapping.analytics_enabled_column))
                .map_err(field("analytics_enabled"))?,
        })
    })
}

fn open(input: &[u8]) -> Result<(csv::Reader<&[u8]>, csv::StringRecord), CsvError> {
    if input.len() > BODY_LIMIT {
        return Err(CsvError);
    }
    let mut reader = csv::Reader::from_reader(input);
    let header = reader.headers().map_err(|_| CsvError)?.clone();
    if !(2..=MAX_COLUMNS).contains(&header.len()) {
        return Err(CsvError);
    }
    Ok((reader, header))
}

fn is_native(header: &csv::StringRecord) -> bool {
    header.iter().take(HEADER.len()).eq(HEADER)
        && (header.len() == HEADER.len()
            || (header.len() == HEADER.len() + 1
                && header.get(HEADER.len()) == Some("description")))
}

fn read_records<T>(
    reader: &mut csv::Reader<&[u8]>,
    mut parse_record: impl FnMut(csv::StringRecord) -> Result<T, FieldError>,
) -> Result<Vec<T>, RowError> {
    let mut records = Vec::new();
    for record in reader.records() {
        if records.len() == MAX_ITEMS {
            return Err(RowError::default());
        }
        let row = Some(records.len() + 1);
        // A malformed record, for example one with a different number of fields, has no field.
        let record = record.map_err(|_| RowError { row, field: None })?;
        let parsed = parse_record(record).map_err(|FieldError(field)| RowError { row, field })?;
        records.push(parsed);
    }
    if records.is_empty() {
        return Err(RowError::default());
    }
    Ok(records)
}

fn mapped_tags(value: &str) -> Result<Vec<String>, CsvError> {
    match value.trim() {
        "" => Ok(Vec::new()),
        value if value.starts_with('[') => serde_json::from_str(value).map_err(|_| CsvError),
        value => Ok(value.split(',').map(str::to_owned).collect()),
    }
}

fn mapped_bool(value: &str) -> Result<bool, CsvError> {
    let value = value.trim();
    if value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false") {
        Ok(false)
    } else if value == "1" || value.eq_ignore_ascii_case("true") {
        Ok(true)
    } else {
        Err(CsvError)
    }
}

pub(super) fn page(rows: Vec<(i64, LinkMetadata)>) -> Result<(Vec<u8>, Option<i64>), CsvError> {
    let first_description = rows
        .iter()
        .take(MAX_ITEMS)
        .position(|(_, item)| !item.description.is_empty());
    let (mut bytes, mut count) = encode_page(&rows, first_description.is_some())?;
    // Keep the legacy header when the first description did not fit on this page.
    if let Some(index) = first_description
        && count <= index
    {
        (bytes, count) = encode_page(&rows[..index], false)?;
    }
    let cursor = (count < rows.len()).then(|| rows[count - 1].0);
    Ok((bytes, cursor))
}

fn encode_page(
    rows: &[(i64, LinkMetadata)],
    has_descriptions: bool,
) -> Result<(Vec<u8>, usize), CsvError> {
    let mut header = HEADER.join(",").into_bytes();
    if has_descriptions {
        header.extend_from_slice(b",description");
    }
    let field_count = HEADER.len() + usize::from(has_descriptions);
    header.push(b'\n');
    let mut included_bytes = header.len();
    let mut writer = csv::WriterBuilder::new()
        .quote_style(csv::QuoteStyle::Always)
        .from_writer(header);
    let version = EXCHANGE_VERSION.to_string();
    let mut count = 0;
    for (_, item) in rows.iter().take(MAX_ITEMS) {
        let tags = serde_json::to_string(&item.tags).map_err(|_| CsvError)?;
        let fields = [
            Cow::Borrowed(version.as_str()),
            protect(&item.host),
            protect(&item.slug),
            protect(&item.target),
            protect(&item.title),
            protect(&tags),
            Cow::Borrowed(if item.enabled { "true" } else { "false" }),
            Cow::Borrowed(if item.analytics_enabled {
                "true"
            } else {
                "false"
            }),
            protect(&item.description),
        ];
        writer
            .write_record(fields.iter().take(field_count).map(|field| field.as_ref()))
            .map_err(|_| CsvError)?;
        writer.flush().map_err(|_| CsvError)?;

        let encoded_bytes = writer.get_ref().len();
        if encoded_bytes > BODY_LIMIT {
            if count == 0 {
                return Err(CsvError);
            }
            break;
        }
        included_bytes = encoded_bytes;
        count += 1;
    }
    let mut output = writer.into_inner().map_err(|_| CsvError)?;
    output.truncate(included_bytes);
    Ok((output, count))
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
            description: String::new(),
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
    fn description_is_optional_and_spreadsheet_safe() {
        let mut item = link("Title");
        item.description = "=1+2\nSecond line".to_owned();
        let (bytes, _) = page(vec![(1, item)]).unwrap();
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        assert_eq!(reader.headers().unwrap().get(8), Some("description"));
        let raw = reader.records().next().unwrap().unwrap();
        assert_eq!(raw.get(8), Some("'=1+2\nSecond line"));
        assert_eq!(parse(&bytes).unwrap()[0].description, "=1+2\nSecond line");
    }

    #[test]
    fn description_header_depends_on_rows_in_the_encoded_page() {
        for (count, long) in [(MAX_ITEMS, false), (8, true)] {
            let mut rows: Vec<_> = (0..count)
                .map(|index| {
                    let mut item = link("Title");
                    if long {
                        item.target.push_str(&"q".repeat(3900));
                    }
                    (index as i64 + 1, item)
                })
                .collect();
            let mut next = link("Next page");
            next.description = "Note on the next page".to_owned();
            if long {
                next.target.push_str(&"q".repeat(3900));
            }
            rows.push((count as i64 + 1, next));
            let (bytes, cursor) = page(rows).unwrap();
            assert_eq!(cursor, Some(count as i64));
            assert_eq!(parse(&bytes).unwrap().len(), count);
            let mut reader = csv::Reader::from_reader(bytes.as_slice());
            assert_eq!(reader.headers().unwrap().len(), HEADER.len());
        }
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

    #[test]
    fn mapped_columns_preserve_quoted_data_and_duplicate_labels_by_position() {
        let input = b"Address,Name,Notes,Name,Labels,Active\r\n\
            https://files.example/A%2fB?x=A+B,old,\"First, line\r\nSecond line\",docs,\"team, releases\",TRUE\r\n";
        let columns = inspect(input).unwrap();
        assert!(!columns.native);
        assert_eq!(columns.columns[1], columns.columns[3]);
        let mapping = ImportQuery {
            target_column: Some(0),
            slug_column: Some(3),
            description_column: Some(2),
            tags_column: Some(4),
            enabled_column: Some(5),
            host: Some("go.example".to_owned()),
            ..Default::default()
        };
        let items = parse_mapped(input, &mapping).unwrap();
        let item = &items[0];
        assert_eq!(item.host, "go.example");
        assert_eq!(item.slug, "docs");
        assert_eq!(item.target, "https://files.example/A%2fB?x=A+B");
        assert_eq!(item.description, "First, line\nSecond line");
        assert_eq!(item.tags, ["team", " releases"]);
        assert!(item.enabled);
        assert!(!item.analytics_enabled);
        assert!(
            parse_mapped(
                input,
                &ImportQuery {
                    slug_column: Some(0),
                    ..mapping
                }
            )
            .is_err()
        );
    }

    #[test]
    fn column_inspection_checks_record_sizes_and_utf8() {
        assert!(inspect(b"Code,Address\n").is_err());
        assert!(inspect(b"Code,Address\nshort\n").is_err());
        assert!(inspect(b"Code,Address,Ignored\nshort,https://example.test/,\xff").is_err());
        let columns = vec!["column"; MAX_COLUMNS + 1].join(",");
        assert!(inspect(columns.as_bytes()).is_err());
    }
}
