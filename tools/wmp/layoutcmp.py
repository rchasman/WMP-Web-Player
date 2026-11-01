"""Compares the element boxes WMP reports (layout.tsv from the WMP workflow's layout task) with the
engine's tree dumps (`webskin --test-skins <skins> <out>`): elements matched by id per window,
placed by their median offset, are listed when their box differs by more than 2 px.

usage: python3 layoutcmp.py <layout.tsv> <test-skins out dir>
"""
import os
import re
import statistics
import sys
import urllib.parse
from collections import defaultdict

tsv, run = sys.argv[1:3]
wmp = defaultdict(dict)
for line in open(tsv, encoding='utf-8', errors='replace'):
    f = line.rstrip('\n').split('\t')
    if len(f) < 6 or f[3] == 'WINDOW' or f[4] == 'empty':
        continue
    wmp[(f[0].lower(), f[1].lower())][f[2].lower()] = tuple(map(int, f[4].split(',')))
ours = {}
for name in os.listdir(run):
    m = re.match(r'(.+?)\.([^.]+)\.tree\.txt$', name)
    if not m:
        continue
    boxes = {}
    for line in open(os.path.join(run, name), encoding='utf-8', errors='replace'):
        b = re.search(r'<(\w+) id=(\S+?)> abs=\((-?[\d.]+),(-?[\d.]+),(-?[\d.]+)x(-?[\d.]+)\)', line)
        if b and 'HIDDEN' not in line:
            boxes[b.group(2).lower()] = (b.group(1), *[round(float(v)) for v in b.groups()[2:]])
    ours[(urllib.parse.unquote(m.group(1)).lower(), m.group(2).lower())] = boxes
compared, differ = 0, []
for key, theirs in sorted(wmp.items()):
    mine = ours.get(key, {})
    common = [i for i in theirs if i in mine]
    if not common:
        continue
    dx = statistics.median(theirs[i][0] - mine[i][1] for i in common)
    dy = statistics.median(theirs[i][1] - mine[i][2] for i in common)
    for i in common:
        (x, y, w, h), (tag, ox, oy, ow, oh) = theirs[i], mine[i]
        compared += 1
        if w and h and max(abs(x - ox - dx), abs(y - oy - dy), abs(w - ow), abs(h - oh)) > 2:
            differ.append(f'{key[0]}/{key[1]}: <{tag} id={i}> WMP {x - dx:.0f},{y - dy:.0f} {w}x{h}  engine {ox},{oy} {ow}x{oh}')
print(f'{compared} elements compared, {len(differ)} differ by more than 2 px')
print('\n'.join(differ))
