import re
import shlex
import struct
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
        self.assertRegex(
            template.findtext("Repository"),
            rf"^ghcr\.io/buffer379x/shortcircuit:{re.escape(version)}-preview\.[1-9][0-9]*$",
        )

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
