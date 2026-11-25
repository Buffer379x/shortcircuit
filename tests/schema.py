import pathlib
import sqlite3
import tempfile
import unittest

MIGRATION = pathlib.Path(__file__).resolve().parents[1] / "migrations" / "0001_links.sql"


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


if __name__ == "__main__":
    unittest.main()
