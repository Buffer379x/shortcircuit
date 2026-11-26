use std::collections::HashMap;

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::domains::Domain;

use super::{Link, PAGE_SIZE, push_host_scope};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    #[default]
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    SlugAsc,
    SlugDesc,
    HostAsc,
    HostDesc,
    TargetAsc,
    TargetDesc,
    EnabledFirst,
    DisabledFirst,
}

impl Sort {
    fn expression(self) -> &'static str {
        match self {
            Self::Newest | Self::Oldest => "created_at",
            Self::TitleAsc | Self::TitleDesc => "title",
            Self::SlugAsc | Self::SlugDesc => "slug",
            Self::HostAsc | Self::HostDesc => "host",
            Self::TargetAsc | Self::TargetDesc => "target",
            Self::EnabledFirst | Self::DisabledFirst => "CAST(enabled AS TEXT)",
        }
    }

    fn descending(self) -> bool {
        matches!(
            self,
            Self::Newest
                | Self::TitleDesc
                | Self::SlugDesc
                | Self::HostDesc
                | Self::TargetDesc
                | Self::EnabledFirst
        )
    }

    fn key(self, link: &Link) -> String {
        match self {
            Self::Newest | Self::Oldest => link.created_at.clone(),
            Self::TitleAsc | Self::TitleDesc => link.title.clone(),
            Self::SlugAsc | Self::SlugDesc => link.slug.clone(),
            Self::HostAsc | Self::HostDesc => link.host.clone(),
            Self::TargetAsc | Self::TargetDesc => link.target.clone(),
            Self::EnabledFirst | Self::DisabledFirst => u8::from(link.enabled).to_string(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) sort: Sort,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Search {
    pub(crate) query: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) sort: Sort,
    pub(crate) after: Option<Cursor>,
}

#[derive(Serialize)]
pub(crate) struct SearchPage {
    items: Vec<SearchItem>,
    next_cursor: Option<Cursor>,
}

#[derive(Serialize)]
struct SearchItem {
    #[serde(flatten)]
    link: Link,
    selection_key: String,
}

#[derive(sqlx::FromRow)]
struct SearchRow {
    #[sqlx(flatten)]
    link: Link,
    selection_key: String,
    tags_json: String,
}

pub(crate) async fn search(
    pool: &SqlitePool,
    domains: &[Domain],
    filter: &Search,
) -> Result<SearchPage, sqlx::Error> {
    let hosts: Vec<_> = domains.iter().map(|domain| domain.host.clone()).collect();
    let origins: HashMap<_, _> = domains
        .iter()
        .map(|domain| (domain.host.as_str(), domain.origin.as_str()))
        .collect();
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id, host, slug, target, created_at, title, enabled, analytics_enabled, selection_key,
                (SELECT json_group_array(tag ORDER BY tag) FROM link_tags WHERE link_id = links.id) AS tags_json
         FROM links WHERE 1",
    );
    push_host_scope(&mut query, &hosts);
    if let Some(host) = &filter.host {
        query.push(" AND host = ").push_bind(host);
    }
    if let Some(enabled) = filter.enabled {
        query.push(" AND enabled = ").push_bind(enabled);
    }
    if let Some(tag) = &filter.tag {
        query
            .push(" AND EXISTS (SELECT 1 FROM link_tags WHERE link_id = links.id AND tag = ")
            .push_bind(tag)
            .push(")");
    }
    let sort = filter.sort;
    let comparison = if sort.descending() { " < " } else { " > " };
    let direction = if sort.descending() { " DESC" } else { " ASC" };
    if let Some(after) = &filter.after {
        query
            .push(" AND (")
            .push(sort.expression())
            .push(" COLLATE NOCASE, id)")
            .push(comparison)
            .push("(")
            .push_bind(&after.key)
            .push(", ")
            .push_bind(after.id)
            .push(")");
    }
    query
        .push(" ORDER BY ")
        .push(sort.expression())
        .push(" COLLATE NOCASE")
        .push(direction)
        .push(", id")
        .push(direction);
    if filter.query.is_none() {
        query.push(" LIMIT ").push_bind((PAGE_SIZE + 1) as i64);
    }
    let mut rows = query.build_query_as::<SearchRow>().fetch(pool);
    let mut items = Vec::with_capacity(PAGE_SIZE + 1);
    while let Some(mut row) = rows.try_next().await? {
        row.link.tags = serde_json::from_str(&row.tags_json)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        let origin = origins.get(row.link.host.as_str()).copied();
        if filter
            .query
            .as_deref()
            .is_none_or(|text| matches_query(&row.link, origin, text))
        {
            items.push(SearchItem {
                link: row.link,
                selection_key: row.selection_key,
            });
            if items.len() > PAGE_SIZE {
                break;
            }
        }
    }
    let has_more = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);
    let next_cursor = items.last().filter(|_| has_more).map(|link| Cursor {
        id: link.link.id,
        key: sort.key(&link.link),
        sort,
    });
    Ok(SearchPage { items, next_cursor })
}

fn matches_query(link: &Link, origin: Option<&str>, text: &str) -> bool {
    let contains = |value: &str| value.contains(text) || value.to_lowercase().contains(text);
    [link.title.as_str(), &link.slug, &link.host, &link.target]
        .into_iter()
        .chain(link.tags.iter().map(String::as_str))
        .any(contains)
        || contains(&format!("{}/{}", link.host, link.slug))
        || origin.is_some_and(|base| contains(&format!("{base}/{}", link.slug)))
}
