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


if __name__ == "__main__":
    unittest.main()
