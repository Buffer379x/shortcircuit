import pathlib
import sqlite3
import tempfile
import unittest

MIGRATION = pathlib.Path(__file__).resolve().parents[1] / "migrations" / "0001_links.sql"
PROTECTION = MIGRATION.with_name("0007_link_protection.sql")
HISTORY = MIGRATION.with_name("0008_link_history.sql")
QR_TEMPLATES = MIGRATION.with_name("0009_qr_templates.sql")
QR_FRAMES = MIGRATION.with_name("0010_qr_frames.sql")
LINK_SELECTION = MIGRATION.with_name("0011_link_selection.sql")
LINK_ORGANIZATION = MIGRATION.with_name("0012_link_organization.sql")
LINK_DESCRIPTION = MIGRATION.with_name("0013_link_description.sql")
TAG_DETAILS = MIGRATION.with_name("0014_tag_details.sql")
CRAWLER_CONTROLS = MIGRATION.with_name("0015_crawler_controls.sql")


class LinkSchemaTests(unittest.TestCase):
    def test_first_migration(self):
        self.assertTrue(MIGRATION.is_file(), "expected a versioned link migration")

    def database(self):
        if not MIGRATION.is_file():
            self.skipTest("first migration not yet present")
        connection = sqlite3.connect(":memory:")
        self.addCleanup(connection.close)
        connection.executescript(MIGRATION.read_text())
        return connection

    @staticmethod
    def insert(connection, host, slug, target="https://example.test/path"):
        connection.execute(
            "INSERT INTO links (host, slug, target) VALUES (?, ?, ?)",
            (host, slug, target),
        )

    def test_slug_is_case_sensitive_and_unique_per_host(self):
        db = self.database()
        self.insert(db, "go.example", "Team/Docs")
        self.insert(db, "go.example", "team/docs")
        self.insert(db, "s.example", "Team/Docs")
        with self.assertRaises(sqlite3.IntegrityError):
            self.insert(db, "go.example", "Team/Docs")

    def test_read_queries_use_the_host_slug_index(self):
        db = self.database()
        result = db.execute(
            "EXPLAIN QUERY PLAN SELECT target FROM links WHERE host = ? AND slug = ?",
            ("go.example", "Team/Docs"),
        ).fetchone()
        self.assertIn("USING INDEX", result[3])
        self.assertIn("host=? AND slug=?", result[3])

    def test_constraints_and_utc_defaults(self):
        db = self.database()
        self.insert(db, "go.example", "docs")
        created_at = db.execute("SELECT created_at FROM links").fetchone()[0]
        self.assertRegex(created_at, r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T.+Z$")
        for host, slug, target in [
            ("", "docs", "https://example.test"),
            ("go.example", "", "https://example.test"),
            ("go.example", "other", ""),
        ]:
            with self.subTest(host=host, slug=slug, target=target):
                with self.assertRaises(sqlite3.IntegrityError):
                    self.insert(db, host, slug, target)

    def test_disk_database_preserves_exact_location_across_reopen(self):
        if not MIGRATION.is_file():
            self.skipTest("first migration not yet present")
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "links.sqlite"
            db = sqlite3.connect(path)
            self.assertEqual(db.execute("PRAGMA journal_mode = WAL").fetchone(), ("wal",))
            db.executescript(MIGRATION.read_text())
            target = "https://example.test/file?Signature=XL%2f&key=A+B&key=C#original"
            self.insert(db, "go.example", "download", target)
            db.commit()
            db.close()
            db = sqlite3.connect(path)
            self.assertEqual(db.execute("SELECT target FROM links").fetchone(), (target,))
            db.close()

    def test_link_protection_schema_preserves_old_links_and_cascades_on_delete(self):
        self.assertTrue(PROTECTION.is_file(), "expected a versioned link protection migration")
        db = self.database()
        db.execute("PRAGMA foreign_keys = ON")
        self.insert(db, "go.example", "existing")
        db.executescript(PROTECTION.read_text())
        link_id = db.execute("SELECT id FROM links WHERE slug = 'existing'").fetchone()[0]
        self.assertIsNone(db.execute("SELECT link_id FROM link_protection").fetchone())
        db.execute("INSERT INTO link_protection (link_id, one_time) VALUES (?, 1)", (link_id,))
        self.assertEqual(
            db.execute("SELECT password_hash, consumed, revision FROM link_protection").fetchone(),
            (None, 0, 1),
        )
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE link_protection SET consumed = 1, one_time = 0")
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE link_protection SET revision = 0")
        db.execute("UPDATE link_protection SET consumed = 1")
        db.execute("DELETE FROM links WHERE id = ?", (link_id,))
        self.assertIsNone(db.execute("SELECT link_id FROM link_protection").fetchone())

    def test_link_history_schema_preserves_identity_and_checks_snapshot_kind(self):
        db = self.database()
        db.execute("PRAGMA foreign_keys = ON")
        self.insert(db, "go.example", "existing")
        db.executescript(HISTORY.read_text())
        link_id = db.execute("SELECT id FROM links").fetchone()[0]
        sql = """INSERT INTO link_history (link_id, actor_kind, actor_label, action, previous)
                 VALUES (?, 'admin', 'owner', ?, ?) RETURNING id"""
        first = db.execute(sql, (link_id, "protection", None)).fetchone()[0]
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute(sql, (link_id, "metadata", None))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute(sql, (link_id, "protection", '{"kind":"metadata","value":{}}'))
        db.execute("DELETE FROM link_history")
        next_id = db.execute(sql, (link_id, "protection", None)).fetchone()[0]
        self.assertGreater(next_id, first)
        db.execute("DELETE FROM links WHERE id = ?", (link_id,))
        self.assertIsNone(db.execute("SELECT id FROM link_history").fetchone())

    def test_qr_template_upgrade_preserves_existing_links_and_checks_styles(self):
        self.assertTrue(QR_TEMPLATES.is_file(), "expected a versioned QR template migration")
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < QR_TEMPLATES:
                db.executescript(migration.read_text())
        self.insert(db, "go.example", "before-upgrade")
        db.executescript(QR_TEMPLATES.read_text())
        self.assertEqual(db.execute("SELECT slug FROM links").fetchone(), ("before-upgrade",))
        insert = """INSERT INTO qr_templates
                    (name, size, foreground, background, transparent, correction, quiet_zone)
                    VALUES (?, 512, ?, 'fff4e0', 1, 'H', 8)"""
        db.execute(insert, ("Print", "002040"))
        for name, color in [("Print", "002040"), ("", "002040"), ("Invalid", "00000g")]:
            with self.subTest(name=name, color=color), self.assertRaises(sqlite3.IntegrityError):
                db.execute(insert, (name, color))
        db.execute("DELETE FROM links")
        self.assertEqual(db.execute("SELECT name FROM qr_templates").fetchone(), ("Print",))

    def test_qr_frame_upgrade_preserves_existing_templates(self):
        self.assertTrue(QR_FRAMES.is_file(), "expected a versioned QR frame migration")
        db = self.database()
        db.executescript(QR_TEMPLATES.read_text())
        db.execute("""INSERT INTO qr_templates
                      (name, size, foreground, background, transparent, correction, quiet_zone)
                      VALUES ('Print', 512, '002040', 'fff4e0', 1, 'H', 8)""")
        db.executescript(QR_FRAMES.read_text())
        self.assertEqual(db.execute("SELECT name, size, frame FROM qr_templates").fetchone(), ("Print", 512, "none"))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE qr_templates SET frame = 'custom'")
        db.execute("UPDATE qr_templates SET frame = 'outline'")
        self.assertEqual(db.execute("SELECT frame FROM qr_templates").fetchone(), ("outline",))

    def test_link_selection_key_changes_when_a_row_id_is_reused(self):
        db = self.database()
        self.insert(db, "go.example", "saved")
        db.executescript(LINK_SELECTION.read_text())
        link_id, key = db.execute("SELECT id, selection_key FROM links").fetchone()
        self.assertRegex(key, r"^[0-9a-f]{32}$")
        db.execute("DELETE FROM links")
        self.insert(db, "go.example", "saved")
        recreated_id, recreated_key = db.execute("SELECT id, selection_key FROM links").fetchone()
        self.assertEqual(recreated_id, link_id)
        self.assertRegex(recreated_key, r"^[0-9a-f]{32}$")
        self.assertNotEqual(recreated_key, key)

    def test_organization_upgrade_preserves_published_links(self):
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < LINK_ORGANIZATION:
                db.executescript(migration.read_text())
        target = "https://example.test/file?Signature=A%2f#original"
        self.insert(db, "go.example", "Team/Docs", target)
        original_key = db.execute("SELECT selection_key FROM links").fetchone()[0]
        self.assertTrue(LINK_ORGANIZATION.is_file(), "expected a versioned organization migration")
        db.executescript(LINK_ORGANIZATION.read_text())
        self.assertEqual(db.execute("SELECT favorite, archived FROM links").fetchone(), (0, 0))
        db.execute("UPDATE links SET favorite = 1, archived = 1")
        self.assertEqual(
            db.execute("SELECT slug, target, enabled, selection_key FROM links").fetchone(),
            ("Team/Docs", target, 1, original_key),
        )
        for field in ("favorite", "archived"):
            with self.subTest(field=field), self.assertRaises(sqlite3.IntegrityError):
                db.execute(f"UPDATE links SET {field} = 2")


    def test_description_upgrade_preserves_target_and_defaults_to_empty(self):
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < LINK_DESCRIPTION:
                db.executescript(migration.read_text())
        target = "https://example.test/file?Signature=A%2f#original"
        self.insert(db, "go.example", "Team/Docs", target)
        self.assertTrue(LINK_DESCRIPTION.is_file(), "expected a versioned description migration")
        db.executescript(LINK_DESCRIPTION.read_text())
        self.assertEqual(db.execute("SELECT target, description FROM links").fetchone(), (target, ""))
        db.execute("UPDATE links SET description = ?", ("ü" * 500,))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE links SET description = ?", ("ü" * 501,))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE links SET description = ?", ("with\x00NUL",))


    def test_tag_details_upgrade_preserves_assignments_and_rejects_malformed_styles(self):
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < TAG_DETAILS:
                db.executescript(migration.read_text())
        self.insert(db, "go.example", "existing")
        db.execute("INSERT INTO link_tags(link_id, tag) VALUES (1, 'work')")
        self.assertTrue(TAG_DETAILS.is_file(), "expected a versioned tag details migration")
        db.executescript(TAG_DETAILS.read_text())
        self.assertEqual(db.execute("SELECT link_id, tag FROM link_tags").fetchall(), [(1, "work")])
        db.execute("INSERT INTO tag_details(name, color, description) VALUES ('work', '#4f9cec', '')")
        for row in [
            ("script", "var(--ink)", "Invalid color"),
            ("nullcolor", "#112233\x00trail", "Invalid color"),
            ("empty", None, ""),
            ("\x00tag", None, "NUL"),
            ("overlong", None, "ü" * 501),
        ]:
            with self.subTest(name=row[0]), self.assertRaises(sqlite3.IntegrityError):
                db.execute("INSERT INTO tag_details(name, color, description) VALUES (?, ?, ?)", row)


    def test_crawler_controls_leave_existing_domains_and_links_permissive(self):
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < CRAWLER_CONTROLS:
                db.executescript(migration.read_text())
        self.insert(db, "go.example", "notes")
        db.execute("INSERT INTO domains (host, origin) VALUES ('go.example', 'https://go.example')")
        db.execute("""INSERT INTO link_redirect_policies
                      (link_id, status, forward_query, forward_path, rules)
                      VALUES (1, 302, 0, 0, '[]')""")
        self.assertTrue(CRAWLER_CONTROLS.is_file(), "expected a versioned crawler controls migration")
        db.executescript(CRAWLER_CONTROLS.read_text())
        self.assertEqual(db.execute("SELECT block_crawlers FROM domains").fetchone(), (0,))
        self.assertEqual(db.execute("SELECT noindex FROM link_redirect_policies").fetchone(), (0,))
        for table, field in [("domains", "block_crawlers"), ("link_redirect_policies", "noindex")]:
            with self.subTest(field=field), self.assertRaises(sqlite3.IntegrityError):
                db.execute(f"UPDATE {table} SET {field} = 2")


    def test_monitor_enrollment_is_removed_when_a_destination_changes(self):
        db = self.database()
        db.execute("PRAGMA foreign_keys = ON")
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if migration > MIGRATION:
                db.executescript(migration.read_text())
        self.insert(db, "go.example", "enrollment", "https://example.test/saved")
        self.assertEqual(db.execute("SELECT count(*) FROM link_monitor").fetchone(), (0,))
        first = db.execute(
            "INSERT INTO link_monitor (link_id, priority) VALUES (1, 1) RETURNING revision"
        ).fetchone()
        db.execute("UPDATE links SET target = target, title = 'Updated'")
        self.assertEqual(db.execute("SELECT revision FROM link_monitor").fetchone(), first)
        db.execute("UPDATE links SET target = 'https://example.test/changed'")
        self.assertEqual(db.execute("SELECT count(*) FROM link_monitor").fetchone(), (0,))
        second = db.execute(
            "INSERT INTO link_monitor (link_id, priority) VALUES (1, 0) RETURNING revision"
        ).fetchone()
        self.assertNotEqual(first, second)
        db.execute("DELETE FROM links")
        self.assertEqual(db.execute("SELECT count(*) FROM link_monitor").fetchone(), (0,))

    def test_domain_deletion_audit_keeps_entries_and_bounds_details(self):
        domain_audit = MIGRATION.with_name("0028_domain_deletion_audit.sql")
        self.assertTrue(domain_audit.is_file(), "expected a versioned domain deletion audit migration")
        db = self.database()
        db.execute("PRAGMA foreign_keys = ON")
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < domain_audit:
                db.executescript(migration.read_text())
        # Every action that the previous schema accepts must stay accepted.
        previous = db.execute(
            "SELECT sql FROM sqlite_schema WHERE name = 'admin_audit'"
        ).fetchone()[0]
        actions = previous.split("action IN (", 1)[1].split(")", 1)[0]
        accepted = [item.strip().strip("'") for item in actions.split(",")]
        for action in accepted:
            db.execute(
                "INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES (?, 'admin', 1, 'success')",
                (action,),
            )
        db.execute("DELETE FROM admin_audit WHERE id > 2")
        before = db.execute("SELECT * FROM admin_audit ORDER BY id").fetchall()
        db.executescript(domain_audit.read_text())
        self.assertEqual(
            db.execute(
                "SELECT id, created_at, action, actor, actor_id, object_id, outcome FROM admin_audit ORDER BY id"
            ).fetchall(),
            before,
        )
        self.assertEqual(db.execute("SELECT count(*) FROM admin_audit WHERE details IS NOT NULL").fetchone(), (0,))
        for action in accepted + ["domain_delete"]:
            with self.subTest(action=action):
                db.execute(
                    "INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES (?, 'admin', 1, 'success')",
                    (action,),
                )
        # The ID sequence carries forward, so a trimmed ID is never issued again.
        self.assertGreater(db.execute("SELECT min(id) FROM admin_audit WHERE id > 2").fetchone()[0], len(accepted))
        db.execute(
            "INSERT INTO admin_audit (action, actor, actor_id, outcome, details) "
            "VALUES ('domain_delete', 'admin', 1, 'success', ?)",
            ('{"host":"old.example","links":"move","target":"new.example","count":2}',),
        )
        for details in ["[]", "1", "{", "{\"x\":\"" + "a" * 1024 + "\"}"]:
            with self.subTest(details=details[:12]), self.assertRaises(sqlite3.IntegrityError):
                db.execute(
                    "INSERT INTO admin_audit (action, actor, actor_id, outcome, details) "
                    "VALUES ('domain_delete', 'admin', 1, 'success', ?)",
                    (details,),
                )
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES ('domain_purge', 'admin', 1, 'success')")
        self.assertEqual(
            db.execute("SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND name = 'admin_audit_trim'").fetchone(),
            (1,),
        )


    def test_public_pages_are_constrained_and_audit_actions_carry_forward(self):
        pages = MIGRATION.with_name("0027_public_pages.sql")
        self.assertTrue(pages.is_file(), "expected a versioned public pages migration")
        db = self.database()
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if MIGRATION < migration < pages:
                db.executescript(migration.read_text())
        previous = db.execute("SELECT sql FROM sqlite_schema WHERE name = 'admin_audit'").fetchone()[0]
        accepted = [item.strip().strip("'") for item in previous.split("action IN (", 1)[1].split(")", 1)[0].split(",")]
        for action in accepted:
            db.execute("INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES (?, 'admin', 1, 'success')", (action,))
        before = db.execute("SELECT * FROM admin_audit ORDER BY id").fetchall()
        db.executescript(pages.read_text())
        self.assertEqual(db.execute("SELECT * FROM admin_audit ORDER BY id").fetchall(), before)
        for action in ["public_pages_update", "public_pages_reset", "public_pages_import"]:
            db.execute("INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES (?, 'admin', 1, 'success')", (action,))
        png = b"\x89PNG\r\n\x1a\n"
        db.execute("INSERT INTO public_pages (page, lang, mode, title, accent, logo_type, logo) "
                   "VALUES ('missing', 'en', 'structured', 'Gone', '#ff79c6', 'image/png', ?)", (png,))
        db.execute("INSERT INTO public_pages (page, lang, mode, html) VALUES ('password', 'de', 'custom', '{{form}}')")
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("INSERT INTO public_pages (page, lang, mode) VALUES ('missing', 'en', 'structured')")
        for columns, values in [
            ("page, lang, mode", ("admin", "en", "structured")),
            ("page, lang, mode", ("missing", "fr", "structured")),
            ("page, lang, mode", ("missing", "de", "builtin")),
            ("page, lang, mode, accent", ("missing", "de", "structured", "#FF79C6")),
            ("page, lang, mode, accent", ("missing", "de", "structured", "red")),
            ("page, lang, mode, logo_type, logo", ("missing", "de", "structured", "image/svg+xml", png)),
            ("page, lang, mode, logo", ("missing", "de", "structured", png)),
            ("page, lang, mode, logo_type, logo", ("missing", "de", "structured", "image/png", b"x" * 65537)),
            ("page, lang, mode", ("missing", "de", "custom")),
            ("page, lang, mode, html, title", ("missing", "de", "custom", "<p>x</p>", "t")),
            ("page, lang, mode, html", ("missing", "de", "structured", "<p>x</p>")),
            ("page, lang, mode, html", ("missing", "de", "custom", "x" * 131073)),
            ("page, lang, mode, button", ("unknown_host", "de", "structured", "Go")),
            ("page, lang, mode, title", ("missing", "de", "structured", "")),
        ]:
            marks = ", ".join("?" for _ in values)
            with self.subTest(values=str(values)[:60]), self.assertRaises(sqlite3.IntegrityError):
                db.execute(f"INSERT INTO public_pages ({columns}) VALUES ({marks})", values)


    def test_runtime_settings_start_unsaved_and_are_range_checked(self):
        # Schema 26: one row whose NULL columns mean "not saved here", so an upgrade changes nothing.
        db = sqlite3.connect(":memory:")
        self.addCleanup(db.close)
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if int(migration.name[:4]) <= 26:
                db.executescript(migration.read_text())
        row = db.execute("SELECT * FROM runtime_settings").fetchall()
        self.assertEqual(len(row), 1)
        self.assertEqual(set(row[0][1:]), {None})
        db.execute(
            "UPDATE runtime_settings SET monitor_enabled = 1, monitor_interval_hours = 12, "
            "monitor_priority_hours = 12, backup_interval_hours = 0, backup_keep = 365, "
            "notification_webhook_url = '', notification_authorization = '', "
            "notification_events = 'security,link_health', notification_click_milestones = '', "
            "notification_expiry_notice_hours = 720"
        )
        db.execute("UPDATE runtime_settings SET notification_webhook_url = 'https://hooks.example/x'")
        for column, value in [
            ("monitor_enabled", 2),
            ("monitor_interval_hours", 0),
            ("monitor_interval_hours", 721),
            ("monitor_priority_hours", 13),
            ("backup_interval_hours", -1),
            ("backup_keep", 0),
            ("notification_webhook_url", "http://hooks.example/"),
            ("notification_events", "Security"),
            ("notification_events", ""),
            ("notification_click_milestones", "10;20"),
            ("notification_expiry_notice_hours", 0),
            ("monitor_enabled", "yes"),
        ]:
            with self.subTest(column=column, value=value), self.assertRaises(sqlite3.IntegrityError):
                db.execute(f"UPDATE runtime_settings SET {column} = ?", (value,))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("INSERT INTO runtime_settings (id) VALUES (2)")
        for action in ("operations_settings_authorize", "operations_settings_update"):
            db.execute("INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES (?, 'admin', 1, 'success')", (action,))

    def test_link_previews_keep_only_bounded_thumbnails_and_follow_the_link(self):
        # Schema 29: one row per link; bytes and metadata only while ready; origin without a path.
        previews = MIGRATION.with_name("0029_link_previews.sql")
        self.assertTrue(previews.is_file(), "expected a versioned link preview migration")
        db = sqlite3.connect(":memory:")
        self.addCleanup(db.close)
        db.execute("PRAGMA foreign_keys = ON")
        for migration in sorted(MIGRATION.parent.glob("*.sql")):
            if migration < previews:
                db.executescript(migration.read_text())
        self.insert(db, "go.example", "one")
        self.insert(db, "go.example", "two")
        db.executescript(previews.read_text())
        self.assertEqual(db.execute("SELECT preview_images_enabled FROM runtime_settings").fetchone(), (None,))
        for value in (2, "yes"):
            with self.subTest(value=value), self.assertRaises(sqlite3.IntegrityError):
                db.execute("UPDATE runtime_settings SET preview_images_enabled = ?", (value,))
        ready = ("INSERT INTO link_previews (link_id, status, kind, mime, bytes, width, height, "
                 "source_origin, fetched_at) VALUES (?, 'ready', ?, 'image/jpeg', ?, ?, ?, ?, 1)")
        db.execute(ready, (1, "image", b"\xff\xd8", 480, 270, "https://cdn.example"))
        for values in [
            (2, "image", b"x" * 49153, 480, 270, "https://cdn.example"),
            (2, "image", b"x", 481, 270, "https://cdn.example"),
            (2, "icon", b"x", 65, 64, "https://cdn.example"),
            (2, "image", b"x", 10, 10, "https://cdn.example/path?token=a"),
            (2, "image", b"x", 10, 10, "https://user@cdn.example"),
            (2, "image", b"x", 10, 10, "ftp://cdn.example"),
            (2, "image", "text", 10, 10, "https://cdn.example"),
            (2, "svg", b"x", 10, 10, "https://cdn.example"),
            (9, "image", b"x", 10, 10, "https://cdn.example"),
        ]:
            with self.subTest(values=values[1:4]), self.assertRaises(sqlite3.IntegrityError):
                db.execute(ready, values)
        for statement in [
            "INSERT INTO link_previews (link_id, bytes) VALUES (2, X'FF')",
            "INSERT INTO link_previews (link_id, status, fetched_at) VALUES (2, 'failed', 1)",
            "INSERT INTO link_previews (link_id, status, failure, fetched_at) VALUES (2, 'failed', 'other', 1)",
            "INSERT INTO link_previews (link_id, mime) VALUES (2, 'image/svg+xml')",
        ]:
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                db.execute(statement)
        # A changed destination clears the thumbnail and queues a new fetch.
        revision = db.execute("SELECT revision FROM link_previews").fetchone()
        db.execute("UPDATE links SET title = 'Same target' WHERE id = 1")
        self.assertEqual(db.execute("SELECT status FROM link_previews").fetchone(), ("ready",))
        db.execute("UPDATE links SET target = 'https://example.test/moved' WHERE id = 1")
        self.assertEqual(
            db.execute("SELECT status, bytes, kind, source_origin, fetched_at FROM link_previews").fetchone(),
            ("pending", None, None, None, None),
        )
        self.assertNotEqual(db.execute("SELECT revision FROM link_previews").fetchone(), revision)
        # Protection removes it, and deleting the link cascades.
        db.execute("INSERT INTO link_previews (link_id) VALUES (2)")
        db.execute("INSERT INTO link_protection (link_id, one_time) VALUES (1, 1)")
        self.assertEqual(db.execute("SELECT link_id FROM link_previews").fetchall(), [(2,)])
        db.execute("DELETE FROM links WHERE id = 2")
        self.assertEqual(db.execute("SELECT count(*) FROM link_previews").fetchone(), (0,))


if __name__ == "__main__":
    unittest.main()
