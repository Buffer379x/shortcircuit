-- Redirect totals per link for a day range, read from the index alone. The catalog's
-- 30-day visit sort, its visit and trend columns, and recorded-use summaries filter by
-- link, kind, and day and only need the total.
CREATE INDEX analytics_daily_by_link_kind ON analytics_daily (link_id, kind, day, total);
