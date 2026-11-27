#!/usr/bin/env python3
"""Checks the central UI catalog: every area file defines the same keys in English and German,
no key is defined in two files, and no translation is empty."""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / 'frontend/src/i18n'
KEY = re.compile(r'^\s{2}(\w+):', re.M)
problems = []
owner = {}
for file in sorted(ROOT.glob('*.ts')):
    text = file.read_text()
    parts = re.split(r'^export const de\b.*$', text, maxsplit=1, flags=re.M)
    if len(parts) != 2:
        problems.append(f'{file.name}: missing German catalog')
        continue
    en, de = (set(KEY.findall(p)) for p in parts)
    for key in sorted(en ^ de):
        problems.append(f'{file.name}: {key} is missing in {"German" if key in en else "English"}')
    for key in en:
        if key in owner:
            problems.append(f'{key} is defined in {owner[key]} and {file.name}')
        owner[key] = file.name
    for key in re.findall(r"^\s{2}(\w+):\s*(?:''|\"\"),?$", text, re.M):
        problems.append(f'{file.name}: {key} is empty')
for p in problems:
    print(p)
print(f'{len(owner)} keys in {len(list(ROOT.glob("*.ts")))} catalog files, {len(problems)} problems')
sys.exit(1 if problems else 0)
