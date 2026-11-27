#!/usr/bin/env python3
"""WCAG 2.2 contrast check for the Polar Signal tokens, read from src/styles/tokens.css.

Every text token must reach 4.5:1 on every surface, field borders 3:1 on the surfaces they sit on,
and the accent button text 4.5:1 on the accent. Exits non-zero on any failure.
"""
import re
import sys
from pathlib import Path

CSS = (Path(__file__).resolve().parent.parent / 'src/styles/tokens.css').read_text()


def block(selector):
    m = re.search(re.escape(selector) + r'\s*\{(.*?)\n\}', CSS, re.S)
    return dict(re.findall(r'--([\w-]+):\s*(#[0-9a-fA-F]{6})', m.group(1)))


def lum(h):
    c = [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    c = [x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def ratio(a, b):
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


themes = {'dark': block(":root,\n[data-theme='dark']"), 'light': block("[data-theme='light']")}
surfaces = ['bg', 'bg-sidebar', 'surface', 'surface-raised', 'surface-hover']
texts = ['text', 'text-secondary', 'text-muted', 'accent', 'success', 'warning', 'error', 'info']
failures = 0
for name, t in themes.items():
    checks = [(fg, bg, 4.5) for fg in texts for bg in surfaces]
    checks += [('border-control', bg, 3.0) for bg in ('bg', 'surface', 'surface-raised')]
    checks += [('on-accent', 'accent', 4.5), ('on-error', 'error', 4.5)]
    for fg, bg, need in checks:
        r = ratio(t[fg], t[bg])
        if r < need:
            failures += 1
            print(f'FAIL {name}: {fg} on {bg} {r:.2f} < {need}')
    print(f'{name}: {len(checks)} pairs checked')
sys.exit(1 if failures else 0)
