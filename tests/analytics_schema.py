import os
import pathlib
import sqlite3
import unittest

MIGRATIONS = pathlib.Path(__file__).resolve().parents[1] / "migrations"
BASE_MIGRATIONS = pathlib.Path(os.environ.get("SHORTCIRCUIT_BASE_MIGRATIONS", MIGRATIONS))
SECONDS_PER_DAY = 86_400
TODAY = 20_100


class AnalyticsSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.execute("PRAGMA foreign_keys = ON")
        self.db.execute("PRAGMA trusted_schema = OFF")
        previous = sorted(BASE_MIGRATIONS.glob("000[1-5]_*.sql"))
        self.assertEqual([int(path.name[:4]) for path in previous], [1, 2, 3, 4, 5])
        for migration in previous:
            self.db.executescript(migration.read_text())
        self.db.executemany(
            "INSERT INTO links (id, host, slug, target) VALUES (?, ?, ?, ?)",
            [
                (7, "go.example", "Team/Doc", "https://example.test/d?Sig=XL%2f#original"),
                (8, "s.example", "team/doc", "https://example.test/other"),
            ],
        )
        self.db.executescript((MIGRATIONS / "0006_analytics.sql").read_text())

    def record(self, link_id, host, day, *, kind="redirect", generation=0, increment=1):
        with self.db:
            inserted = self.db.execute(
                """INSERT INTO analytics_events
                       (occurred_at, link_id, host, kind, visit_class, source, referrer)
                   SELECT ?, ?, ?, ?, 'human', 'browser', 'referrer.example'
                   FROM analytics_settings WHERE id = 1 AND generation = ?""",
                (day * SECONDS_PER_DAY + 20, link_id, host, kind, generation),
            )
            if inserted.rowcount == 0:
                return False
            self.db.execute(
                """INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
                   VALUES (?, ?, ?, ?, 'human', ?)
                   ON CONFLICT (day, host, link_id, kind, visit_class)
                   DO UPDATE SET total = total + excluded.total""",
                (day, host, link_id or 0, kind, increment),
            )
        return True

    def test_upgrade_preserves_link_identity_and_sets_privacy_defaults(self):
        link = self.db.execute(
            "SELECT host, slug, target, analytics_enabled FROM links WHERE id = 7"
        ).fetchone()
        self.assertEqual(
            link, ("go.example", "Team/Doc", "https://example.test/d?Sig=XL%2f#original", 1)
        )
        settings = self.db.execute(
            """SELECT enabled, referrers, classify, honor_dnt, detail_days,
                      aggregate_days, generation FROM analytics_settings WHERE id = 1"""
        ).fetchall()
        self.assertEqual(settings, [(1, 1, 1, 1, 90, 730, 0)])
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO analytics_settings (id) VALUES (2)")

    def test_rollup_conflict_increments_total_and_failed_write_rolls_back_event(self):
        self.assertTrue(self.record(7, "go.example", TODAY))
        self.assertTrue(self.record(7, "go.example", TODAY))
        with self.assertRaises(sqlite3.IntegrityError):
            self.record(7, "go.example", TODAY, increment=0)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM analytics_events").fetchone(), (2,))
        self.assertEqual(self.db.execute("SELECT total FROM analytics_daily").fetchall(), [(2,)])

    def test_detail_expiry_preserves_rollups_until_their_own_cutoff(self):
        for day in [TODAY - 750, TODAY - 100, TODAY - 90, TODAY]:
            self.assertTrue(self.record(7, "go.example", day))
        with self.db:
            self.db.execute(
                "DELETE FROM analytics_events WHERE occurred_at < ?",
                ((TODAY - 90) * SECONDS_PER_DAY,),
            )
            self.db.execute("DELETE FROM analytics_daily WHERE day < ?", (TODAY - 730,))
        self.assertEqual(
            self.db.execute("SELECT occurred_at FROM analytics_events ORDER BY occurred_at").fetchall(),
            [((TODAY - 90) * SECONDS_PER_DAY + 20,), (TODAY * SECONDS_PER_DAY + 20,)],
        )
        self.assertEqual(
            self.db.execute("SELECT day FROM analytics_daily ORDER BY day").fetchall(),
            [(TODAY - 100,), (TODAY - 90,), (TODAY,)],
        )

    def test_clear_rejects_old_generation_and_keeps_settings_and_links(self):
        with self.db:
            self.db.execute("UPDATE analytics_settings SET referrers = 0 WHERE id = 1")
        self.assertTrue(self.record(7, "go.example", TODAY))
        with self.db:
            self.db.execute("DELETE FROM analytics_events")
            self.db.execute("DELETE FROM analytics_daily")
            self.db.execute("UPDATE analytics_settings SET generation = generation + 1 WHERE id = 1")
        self.assertFalse(self.record(7, "go.example", TODAY))
        self.assertTrue(self.record(7, "go.example", TODAY, generation=1))
        self.assertEqual(
            self.db.execute("SELECT referrers, generation FROM analytics_settings").fetchone(),
            (0, 1),
        )
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM links").fetchone(), (2,))
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM analytics_events").fetchone(), (1,))
        self.assertEqual(self.db.execute("SELECT total FROM analytics_daily").fetchall(), [(1,)])

    def test_link_deletion_purges_its_detail_and_rollups_only(self):
        self.assertTrue(self.record(7, "go.example", TODAY))
        self.assertTrue(self.record(8, "s.example", TODAY, kind="blocked"))
        self.assertTrue(self.record(None, "go.example", TODAY, kind="missing"))
        self.db.execute("DELETE FROM links WHERE id = 7")
        self.assertEqual(
            self.db.execute("SELECT link_id FROM analytics_events ORDER BY id").fetchall(),
            [(8,), (None,)],
        )
        self.assertEqual(
            self.db.execute("SELECT link_id FROM analytics_daily ORDER BY link_id").fetchall(),
            [(0,), (8,)],
        )
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_detail_restricts_referrer_to_bare_ascii_host_characters(self):
        insert = """INSERT INTO analytics_events
                         (occurred_at, host, kind, visit_class, source, referrer)
                    VALUES (1000, 'go.example', 'missing', 'unknown', 'unknown', ?)"""
        for referrer in [
            "https://referrer.example",
            "referrer.example/private",
            "referrer.example:8443",
            "secret@referrer.example",
            "referrer.example?secret=1",
            "referrer.example\x00private",
            "rèferrer.example",
        ]:
            with self.subTest(referrer=referrer), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(insert, (referrer,))
        self.db.execute(insert, ("referrer.example",))
        self.db.execute(insert, ("[::1]",))
        self.db.execute(insert, (None,))

    def test_dimensions_upgrade_preserves_history_and_its_link_deletion(self):
        self.record(7, "go.example", TODAY, increment=8)
        self.record(None, "s.example", TODAY, kind="missing")
        self.db.executescript((MIGRATIONS / "0016_analytics_dimensions.sql").read_text())
        self.assertEqual(
            self.db.execute(
                "SELECT source, browser, os, device, traffic_source FROM analytics_events "
                "WHERE link_id = 7"
            ).fetchone(),
            ("browser", "unknown", "unknown", "unknown", "unknown"),
        )
        self.assertEqual(
            self.db.execute(
                "SELECT total, browser, os, device, traffic_source FROM analytics_daily "
                "WHERE link_id = 7"
            ).fetchone(),
            (8, "unknown", "unknown", "unknown", "unknown"),
        )
        for category, total in [("referral", 3), ("direct", 6)]:
            self.db.execute(
                """INSERT INTO analytics_daily
                       (day, host, link_id, kind, visit_class, total,
                        browser, os, device, traffic_source)
                   VALUES (?, 'go.example', 7, 'redirect', 'human', ?,
                           'chrome', 'android', 'mobile', ?)""",
                (TODAY, total, category),
            )
        self.assertEqual(
            self.db.execute("SELECT SUM(total) FROM analytics_daily WHERE link_id = 7").fetchone(),
            (17,),
        )
        self.db.execute("DELETE FROM links WHERE id = 7")
        self.assertEqual(self.db.execute("SELECT link_id FROM analytics_daily").fetchall(), [(0,)])
        self.assertEqual(self.db.execute("SELECT link_id FROM analytics_events").fetchall(), [(None,)])
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_dimensions_accept_only_coarse_categories_in_both_tables(self):
        self.db.executescript((MIGRATIONS / "0016_analytics_dimensions.sql").read_text())
        for field, value in [("browser", "Mozilla/5.0"), ("os", "Android 14"),
                             ("device", "Pixel 8"), ("traffic_source", "192.0.2.16")]:
            for table, columns, row in [
                ("analytics_events", "occurred_at, host, kind, visit_class, source",
                 "1000, 'go.example', 'missing', 'unknown', 'unknown'"),
                ("analytics_daily", "day, host, kind, visit_class, link_id, total",
                 "1, 'go.example', 'missing', 'unknown', 0, 1"),
            ]:
                with self.subTest(table=table, field=field), self.assertRaises(sqlite3.IntegrityError):
                    self.db.execute(f"INSERT INTO {table} ({columns}, {field}) VALUES ({row}, ?)",
                                    (value,))


    def test_retention_and_filtered_aggregates_use_range_indexes(self):
        queries = [
            (
                "SELECT id FROM analytics_events WHERE occurred_at < ? ORDER BY occurred_at, id",
                (TODAY * SECONDS_PER_DAY,),
                "analytics_events_by_time",
            ),
            (
                "SELECT total FROM analytics_daily WHERE host = ? AND day >= ?",
                ("go.example", TODAY),
                "analytics_daily_by_host",
            ),
            (
                "SELECT total FROM analytics_daily WHERE link_id = ? AND day >= ?",
                (7, TODAY),
                "analytics_daily_by_link",
            ),
        ]
        for query, bound, index in queries:
            with self.subTest(index=index):
                plan = self.db.execute("EXPLAIN QUERY PLAN " + query, bound).fetchall()
                self.assertTrue(any(index in row[3] for row in plan), plan)


if __name__ == "__main__":
    unittest.main()
