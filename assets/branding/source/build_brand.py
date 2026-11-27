#!/usr/bin/env python3
"""Build the Polar Signal brand SVGs from one geometry source.

The mark, the small-size mark, and the Geist wordmark outlines are defined once here.
Every SVG in ../svg is generated; edit this file, not the outputs.
Writes ../svg/*.svg and ../raster.json (the PNG jobs for rasterize.cjs).
"""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "svg"
OUT.mkdir(exist_ok=True)
WORD = json.loads((HERE / "wordmark.json").read_text())

PINK = "#FF79C6"        # dark-mode brand
PINK_LIGHT = "#B52B7C"  # light-mode brand
INK = "#F6F6FA"         # dark-mode text
INK_LIGHT = "#20222A"   # light-mode text
TILE = "#1C1E24"        # app icon tile
BG = "#101115"

# Primary mark on a 64-unit grid: two circuit traces with round terminals, bridged by the bolt.
# The trace ends are cut parallel to the bolt edges with an equal gap. The figure is
# point-symmetric about (32, 32). Visual bounds: x 10..54, y 8..56.
MARK = (
    '<path d="M33.07 10H20A10 10 0 0 0 10 20v7a3.5 3.5 0 0 0 7 0v-7a3 3 0 0 1 3-3h7.06z"/>'
    '<path d="M30.93 54H44a10 10 0 0 0 10-10v-7a3.5 3.5 0 0 0-7 0v7a3 3 0 0 1-3 3h-7.06z"/>'
    '<path d="M39 8 15 36h15l-5 20 24-28H34z"/>'
)
# Pixel-hinted mark for 16 to 32 px: a larger bolt and shortened, heavier traces.
MARK_SMALL = (
    '<path d="M9 1.5 3.5 8.5H7L5.8 14.5 12.5 7H9z"/>'
    '<path d="M7.2 2.5H4.6A2.1 2.1 0 0 0 2.5 4.6V6h1.6V4.6c0-.3.2-.5.5-.5h1.4z"/>'
    '<path d="M8.8 13.5h2.6a2.1 2.1 0 0 0 2.1-2.1V10h-1.6v1.4c0 .3-.2.5-.5.5H10.2z"/>'
)


def svg(view, body, label="ShortCircuit"):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" role="img" '
            f'aria-label="{label}">{body}</svg>\n')


def mark(fill, view="0 0 64 64"):
    return svg(view, f'<g fill="{fill}">{MARK}</g>')


def app_icon(rounded=True, scale=0.74, bg=TILE, fill=PINK):
    rx = ' rx="14"' if rounded else ""
    return svg("0 0 64 64", f'<rect width="64" height="64"{rx} fill="{bg}"/>'
               f'<g fill="{fill}" transform="translate(32 32) scale({scale}) translate(-32 -32)">{MARK}</g>')


def small_icon(bg=TILE, fill=PINK):
    return svg("0 0 16 16", f'<rect width="16" height="16" rx="3.5" fill="{bg}"/><g fill="{fill}">{MARK_SMALL}</g>')


def horizontal(mark_fill, text_fill, tagline=False):
    # Mark at its visual bounds (44 x 48), cap height 31 units, 16-unit gap.
    s = 0.436
    word_x = 60
    body = f'<g fill="{mark_fill}" transform="translate(-10 -8)">{MARK}</g>'
    body += f'<path fill="{text_fill}" transform="translate({word_x} 4.6) scale({s})" d="{WORD["name"]["d"]}"/>'
    width = word_x + WORD["name"]["width"] * s
    height = 48
    if tagline:
        ts = 0.17
        body += (f'<path fill="{text_fill}" fill-opacity=".72" transform="translate({word_x} 50) scale({ts})" '
                 f'd="{WORD["tagline"]["d"]}"/>')
        height = 50 + 100 * ts
        width = max(width, word_x + WORD["tagline"]["width"] * ts)
    return svg(f"0 0 {width + 1:.0f} {height:.0f}", body)


def stacked(mark_fill, text_fill):
    s = 0.40
    word_w = WORD["name"]["width"] * s
    ts = 0.16
    tag_w = WORD["tagline"]["width"] * ts
    width = max(word_w, tag_w, 96) + 8
    cx = width / 2
    body = f'<g fill="{mark_fill}" transform="translate({cx - 32} 0) scale(1)">{MARK}</g>'
    body += f'<path fill="{text_fill}" transform="translate({cx - word_w / 2} 70) scale({s})" d="{WORD["name"]["d"]}"/>'
    body += (f'<path fill="{text_fill}" fill-opacity=".72" transform="translate({cx - tag_w / 2} 118) scale({ts})" '
             f'd="{WORD["tagline"]["d"]}"/>')
    return svg(f"0 0 {width:.0f} 138", body)


def wordmark(fill):
    return svg(f"0 0 {WORD['name']['width'] + 1:.0f} 74",
               f'<path fill="{fill}" transform="translate(0 -6)" d="{WORD["name"]["d"]}"/>')


def banner(width, height, scale):
    # Repository banner: horizontal lockup with tagline, and two quiet traces bridged by one pink pad.
    logo_w = 60 + WORD["tagline"]["width"] * 0.17
    x = width * 0.08
    y = height / 2 - 33 * scale
    # Two grey detours and the direct route in pink meet at one terminal: the shorter route.
    px, py = width * 0.6, height / 2
    r = height * 0.12
    top, bottom = height * 0.22, height * 0.78
    traces = (
        f'<g fill="none" stroke="#333640" stroke-width="3" stroke-linecap="round">'
        f'<path d="M{width:.0f} {top:.1f}H{px + r + 60:.1f}a{r:.1f} {r:.1f} 0 0 0-{r:.1f} {r:.1f}V{py - r:.1f}'
        f'a{r:.1f} {r:.1f} 0 0 1-{r:.1f} {r:.1f}H{px:.1f}"/>'
        f'<path d="M{width:.0f} {bottom:.1f}H{px + r + 60:.1f}a{r:.1f} {r:.1f} 0 0 1-{r:.1f}-{r:.1f}V{py + r:.1f}'
        f'a{r:.1f} {r:.1f} 0 0 0-{r:.1f}-{r:.1f}H{px:.1f}"/></g>'
        f'<path d="M{width:.0f} {py:.1f}H{px:.1f}" stroke="{PINK}" stroke-width="3" stroke-linecap="round"/>'
        f'<circle cx="{px:.1f}" cy="{py:.1f}" r="7" fill="{PINK}"/>'
    )
    lockup = horizontal(PINK, INK, tagline=True)
    inner = lockup.split(">", 1)[1].rsplit("</svg>", 1)[0]
    return svg(f"0 0 {width} {height}",
               f'<rect width="{width}" height="{height}" fill="{BG}"/>{traces}'
               f'<g transform="translate({x:.0f} {y:.0f}) scale({scale})">{inner}</g>')


files = {
    "mark-dark.svg": mark(PINK),
    "mark-light.svg": mark(PINK_LIGHT),
    "mark-mono-white.svg": mark("#FFFFFF"),
    "mark-mono-black.svg": mark("#000000"),
    "mark-current-color.svg": mark("currentColor"),
    "logo-horizontal-dark.svg": horizontal(PINK, INK),
    "logo-horizontal-light.svg": horizontal(PINK_LIGHT, INK_LIGHT),
    "logo-horizontal-mono-white.svg": horizontal("#FFFFFF", "#FFFFFF"),
    "logo-horizontal-mono-black.svg": horizontal("#000000", "#000000"),
    "logo-tagline-dark.svg": horizontal(PINK, INK, tagline=True),
    "logo-tagline-light.svg": horizontal(PINK_LIGHT, INK_LIGHT, tagline=True),
    "logo-stacked-dark.svg": stacked(PINK, INK),
    "logo-stacked-light.svg": stacked(PINK_LIGHT, INK_LIGHT),
    "wordmark-dark.svg": wordmark(INK),
    "wordmark-light.svg": wordmark(INK_LIGHT),
    "app-icon.svg": app_icon(),
    "app-icon-square.svg": app_icon(rounded=False, scale=0.7),
    "app-icon-maskable.svg": app_icon(rounded=False, scale=0.58),
    "favicon.svg": small_icon(),
    "readme-banner.svg": banner(1280, 320, 1.55),
    "social-preview.svg": banner(1280, 640, 2.0),
}
for name, content in files.items():
    (OUT / name).write_text(content)

# PNG jobs: [svg, out, width, height, transparent]
jobs = []
for size in (16, 24, 32):
    jobs.append(["favicon.svg", f"favicon-{size}.png", size, size, True])
for size in (48, 64, 128, 256, 512, 1024):
    jobs.append(["app-icon.svg", f"app-icon-{size}.png", size, size, True])
jobs += [
    ["app-icon-square.svg", "apple-touch-icon-180.png", 180, 180, False],
    ["app-icon.svg", "pwa-192.png", 192, 192, True],
    ["app-icon.svg", "pwa-512.png", 512, 512, True],
    ["app-icon-maskable.svg", "pwa-maskable-512.png", 512, 512, False],
    ["app-icon.svg", "unraid-docker.png", 512, 512, True],
    ["mark-dark.svg", "mark-dark-512.png", 512, 512, True],
    ["mark-light.svg", "mark-light-512.png", 512, 512, True],
    ["logo-horizontal-dark.svg", "logo-horizontal-dark.png", 1200, 0, True],
    ["logo-horizontal-light.svg", "logo-horizontal-light.png", 1200, 0, True],
    ["logo-stacked-dark.svg", "logo-stacked-dark.png", 800, 0, True],
    ["logo-stacked-light.svg", "logo-stacked-light.png", 800, 0, True],
    ["readme-banner.svg", "readme-banner.png", 1280, 320, False],
    ["social-preview.svg", "social-preview.png", 1280, 640, False],
]
(HERE.parent / "raster.json").write_text(json.dumps(jobs, indent=1))
print(len(files), "svg,", len(jobs), "png jobs")
