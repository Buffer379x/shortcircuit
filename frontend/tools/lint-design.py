#!/usr/bin/env python3
"""Design consistency check for the Polar Signal frontend.

Fails when a component or page:
- uses a raw color literal (hex, rgb, hsl) outside src/styles/tokens.css,
- sets a font-size, border-radius, or z-index with a literal instead of a token,
- defines its own .btn, .badge, .input, or .panel styles outside src/styles/ui.css.
Exceptions are marked on the same line with `design-lint: allow` and a reason.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / 'src'
COLOR = re.compile(r'#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(')
LITERAL = re.compile(r'\b(font-size|border-radius|z-index)\s*:\s*([^;]+);')
OK_VALUE = re.compile(r'^(calc\(var.*|inherit|50%|-?[012]|((var\([^)]*\)|0)\s*)+)$')
OWN = re.compile(r'^\s*\.(btn|badge|input|panel|select|chip|tag)(?![\w-])[^{]*\{', re.M)
ALLOW = 'design-lint: allow'
problems = []

for f in sorted(ROOT.rglob('*')):
    if f.suffix not in {'.css', '.svelte', '.ts'} or f.name == 'icons.ts':
        continue
    rel = f.relative_to(ROOT.parent)
    text = f.read_text()
    css = text if f.suffix == '.css' else ''.join(re.findall(r'<style[^>]*>(.*?)</style>', text, re.S))
    if f.name != 'tokens.css':
        for n, line in enumerate(text.splitlines(), 1):
            if ALLOW in line or line.strip().startswith(('//', '*', '/*')):
                continue
            if COLOR.search(line) and 'url("data:image/svg' not in line and "url(\"data:image" not in line:
                problems.append(f'{rel}:{n}: color literal: {line.strip()[:90]}')
    for m in LITERAL.finditer(css):
        line = css[: m.start()].count('\n') + 1
        value = m.group(2).strip()
        tail = css[m.end(): css.find('\n', m.end())]
        if f.name != 'tokens.css' and not OK_VALUE.match(value) and ALLOW not in tail:
            problems.append(f'{rel}: literal {m.group(1)} "{m.group(2).strip()}" near style line {line}')
    if f.suffix == '.svelte':
        for m in OWN.finditer(css):
            problems.append(f'{rel}: redefines shared component class .{m.group(1)}')

for p in problems:
    print(p)
print(f'{len(problems)} design consistency problems')
sys.exit(1 if problems else 0)
