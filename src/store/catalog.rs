use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

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
    items: Vec<Link>,
    next_cursor: Option<Cursor>,
}

pub(crate) async fn search(
    pool: &SqlitePool,
    hosts: &[String],
    filter: &Search,
) -> Result<SearchPage, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id, host, slug, target, created_at, title, enabled, analytics_enabled FROM links WHERE 1",
    );
    push_host_scope(&mut query, hosts);
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
    if let Some(text) = &filter.query {
        query.push(" AND (");
        for (index, expression) in ["title", "slug", "host", "target", "host || '/' || slug"]
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                query.push(" OR ");
            }
            query
                .push("instr(lower(")
                .push(expression)
                .push("), ")
                .push_bind(text)
                .push(") > 0");
        }
        query
            .push(" OR EXISTS (SELECT 1 FROM link_tags WHERE link_id = links.id AND instr(lower(tag), ")
            .push_bind(text)
            .push(") > 0)");
        if let Ok(url) = url::Url::parse(text)
            && matches!(url.scheme(), "http" | "https")
            && let Some(host) = url.host_str()
        {
            query
                .push(" OR lower(host || '/' || slug) = ")
                .push_bind(format!("{host}{}", url.path()));
        }
        query.push(")");
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
        .push(direction)
        .push(" LIMIT ")
        .push_bind((PAGE_SIZE + 1) as i64);
    let mut transaction = pool.begin().await?;
    let mut items = query
        .build_query_as::<Link>()
        .fetch_all(&mut *transaction)
        .await?;
    let has_more = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);
    let positions: HashMap<_, _> = items.iter().enumerate().map(|(pos, link)| (link.id, pos)).collect();
    if !positions.is_empty() {
        let mut tags = QueryBuilder::<Sqlite>::new("SELECT link_id, tag FROM link_tags WHERE link_id IN (");
        let mut ids = tags.separated(", ");
        for id in positions.keys() {
            ids.push_bind(id);
        }
        ids.push_unseparated(") ORDER BY link_id, tag");
        for (id, tag) in tags.build_query_as::<(i64, String)>().fetch_all(&mut *transaction).await? {
            items[positions[&id]].tags.push(tag);
        }
    }
    transaction.commit().await?;
    let next_cursor = items.last().filter(|_| has_more).map(|link| Cursor {
        id: link.id,
        key: sort.key(link),
        sort,
    });
    Ok(SearchPage { items, next_cursor })
}
