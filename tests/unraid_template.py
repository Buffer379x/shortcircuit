import json
import shlex
import struct
import tomllib
import unittest
from pathlib import Path
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = ROOT / "unraid" / "shortcircuit.xml"
BRANDING = ROOT / "assets" / "branding"


class UnraidTemplate(unittest.TestCase):
    def template(self):
        self.assertTrue(TEMPLATE.is_file(), "the Unraid XML template must be included")
        return ElementTree.parse(TEMPLATE).getroot()

    def test_published_image_and_loopback_listeners(self):
        template = self.template()
        self.assertEqual(template.tag, "Container")
        self.assertEqual(template.get("version"), "2")
        self.assertEqual(template.findtext("Network"), "bridge")
        self.assertEqual(template.findtext("Privileged"), "false")
        version = (ROOT / "Dockerfile").read_text().split('org.opencontainers.image.version="')[1].split('"')[0]
        self.assertRegex(version, r"^\d+\.\d+\.\d+-preview\.[1-9][0-9]*$")
        self.assertEqual(template.findtext("Repository"), f"ghcr.io/buffer379x/shortcircuit:{version}")
        self.assertEqual(tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"], version)
        self.assertEqual(json.loads((ROOT / "frontend" / "package.json").read_text())["version"], version)
        self.assertIn(f"image: shortcircuit:{version}\n", (ROOT / "compose.yaml").read_text())

        params = shlex.split(template.findtext("ExtraParams"))
        self.assertEqual(
            {arg for arg in params if arg.startswith("--publish=")},
            {"--publish=127.0.0.1:8080:8080/tcp", "--publish=127.0.0.1:8081:8081/tcp"},
        )
        self.assertFalse(template.findall("./Config[@Type='Port']"))
        self.assertIn("--read-only", params)
        self.assertIn("--cap-drop=ALL", params)
        self.assertIn("--security-opt=no-new-privileges=true", params)
        self.assertEqual(template.findtext("WebUI"), template.findtext("./Config[@Target='ADMIN_ORIGIN']"))
        self.assertIn("--volume=shortcircuit-data:/data", params)
        self.assertEqual(
            template.findtext("ReadMe"),
            f"https://raw.githubusercontent.com/Buffer379x/shortcircuit/v{version}/docs/UNRAID_INSTALLATION.md",
        )
        variables = {config.get("Target"): config.text or "" for config in template.findall("./Config[@Type='Variable']")}
        self.assertEqual(variables["TRUSTED_PROXIES"], "")
        # Operational settings are set in Settings > Operations, not in the template. The built-in
        # backup default matches what the template used to set: daily, keeping seven.
        for name in (
            "BACKUP_INTERVAL_HOURS", "BACKUP_KEEP", "MONITOR_ENABLED", "TITLE_LOOKUP_ENABLED", "PREVIEW_IMAGES_ENABLED",
            "NOTIFICATION_WEBHOOK_URL", "NOTIFICATION_WEBHOOK_URL_FILE", "NOTIFICATION_EVENTS",
        ):
            self.assertNotIn(name, variables)
        runtime = (ROOT / "src" / "runtime_settings.rs").read_text()
        self.assertIn("const DEFAULT_BACKUP_INTERVAL: u16 = 24;", runtime)
        self.assertIn("const DEFAULT_BACKUP_KEEP: u16 = 7;", runtime)
        self.assertIsNone(template.find("./Config[@Target='/data']"))

    def test_template_references_the_branding_image(self):
        template = self.template()
        self.assertEqual(
            template.findtext("Icon"),
            "https://raw.githubusercontent.com/Buffer379x/shortcircuit/main/assets/branding/unraid-docker.png",
        )
        icon = (BRANDING / "unraid-docker.png").read_bytes()
        self.assertEqual(icon[:16], b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")
        self.assertEqual(struct.unpack(">II", icon[16:24]), (256, 256))

    def test_browser_has_local_svg_and_png_favicons(self):
        index = (ROOT / "frontend" / "index.html").read_text()
        self.assertIn('rel="icon" type="image/png" sizes="32x32" href="/favicon.png"', index)
        self.assertIn('rel="icon" type="image/svg+xml" href="/favicon.svg"', index)
        for name in ("favicon.png", "favicon.svg"):
            self.assertEqual(
                (BRANDING / name).read_bytes(),
                (ROOT / "frontend" / "public" / name).read_bytes(),
            )


if __name__ == "__main__":
    unittest.main()
