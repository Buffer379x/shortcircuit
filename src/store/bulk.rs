use serde::Deserialize;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

use super::{Link, push_host_scope};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkSelection {
    pub(crate) id: i64,
    pub(crate) key: String,
}

#[derive(sqlx::FromRow)]
struct SelectedRow {
    #[sqlx(flatten)]
    link: Link,
    tags_json: String,
    protected: bool,
}

pub(crate) async fn read_selected(
    connection: &mut SqliteConnection,
    hosts: &[String],
    selected: &[LinkSelection],
) -> Result<Vec<(Link, bool)>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("WITH selected(id, instance) AS (");
    query.push_values(selected, |mut values, item| {
        values.push_bind(item.id).push_bind(&item.key);
    });
    query.push(
        ") SELECT links.id, host, slug, target, created_at, title, description, enabled, analytics_enabled,
                  (SELECT json_group_array(tag ORDER BY tag) FROM link_tags WHERE link_id = links.id) AS tags_json,
                  EXISTS (
                      SELECT 1 FROM link_protection WHERE link_id = links.id
                      AND (one_time = 1 OR password_hash IS NOT NULL)
                  ) AS protected
           FROM selected JOIN links ON links.id = selected.id AND links.selection_key = selected.instance
           WHERE 1",
    );
    push_host_scope(&mut query, hosts);
    query.push(" ORDER BY links.id");
    query
        .build_query_as::<SelectedRow>()
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(|mut row| {
            row.link.tags = serde_json::from_str(&row.tags_json)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            Ok((row.link, row.protected))
        })
        .collect()
}

pub(crate) async fn delete_selected(
    connection: &mut SqliteConnection,
    selected: &[LinkSelection],
) -> Result<u64, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("DELETE FROM links WHERE id IN (");
    let mut ids = query.separated(", ");
    for item in selected {
        ids.push_bind(item.id);
    }
    ids.push_unseparated(")");
    Ok(query.build().execute(connection).await?.rows_affected())
}
