"""Outline the ShortCircuit wordmark and tagline from Geist with HarfBuzz shaping.

Usage: python3 wordmark.py <Geist-SemiBold.ttf> <Geist-Regular.ttf> <out.json>
Writes SVG path data in font units scaled to a 1000-unit em, plus advance widths.
"""
import json
import sys

import uharfbuzz as hb
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont


def outline(path, text, tracking, size):
    blob = hb.Blob.from_file_path(path)
    face = hb.Face(blob)
    font = hb.Font(face)
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(font, buf, {"kern": True, "liga": True})
    tt = TTFont(path)
    glyphs = tt.getGlyphSet()
    order = tt.getGlyphOrder()
    upem = tt["head"].unitsPerEm
    scale = size / upem
    pen = SVGPathPen(glyphs, lambda v: f"{v:.2f}".rstrip("0").rstrip("."))
    x = 0.0
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        name = order[info.codepoint]
        # Flip the y axis and place the baseline at `size`.
        t = TransformPen(pen, (scale, 0, 0, -scale, (x + pos.x_offset) * scale, size * 0.8))
        glyphs[name].draw(t)
        x += pos.x_advance + tracking * upem
    width = (x - tracking * upem) * scale
    return pen.getCommands(), round(width, 2)


semibold, regular, out = sys.argv[1:4]
name_path, name_w = outline(semibold, "ShortCircuit", -0.022, 100)
tag_path, tag_w = outline(regular, "Take the shorter route.", 0.0, 100)
json.dump({"name": {"d": name_path, "width": name_w, "em": 100},
           "tagline": {"d": tag_path, "width": tag_w, "em": 100}}, open(out, "w"))
print(name_w, tag_w)
