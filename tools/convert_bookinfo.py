#!/usr/bin/env python3
"""Convert the hand-curated table in appxi-cbeta's BookInfo.java into JSON.

  python tools/convert_bookinfo.py path/to/BookInfo.java data/bookinfo.json

Each entry:  "T0001": {kind, kind_label, title, periods[], authors[], raw_author}
The Java file is not part of this repository; data/bookinfo.json is the result
(4826 books). Re-run only if the Java table changes.
"""
import json
import re
import sys
from collections import Counter

def strings(s):
    return re.findall(r'"((?:[^"\\]|\\.)*)"', s)

def convert(java_path):
    src = open(java_path, encoding="utf-8").read()
    labels = dict(re.findall(r'^\s*(\w+)\("([^"]*)"\)[,;]', src, re.M))
    entries, bad = {}, []
    pat = re.compile(r'"(\w+)",\s*Kind\.(\w+)\.of([234])\((.*)\)\s*,?\s*(?://\s*(.*))?$')
    for ln, line in enumerate(src.splitlines(), 1):
        if "Kind." not in line or not re.match(r'\s*"\w+",', line):
            continue
        m = pat.match(line.strip())
        if not m:
            bad.append((ln, line.strip()))
            continue
        bid, kind, n, args, comment = m.groups()
        n = int(n)
        if n == 2:                       # of2(title, authors...)
            t = strings(args)
            title, periods, authors = t[0], [], t[1:]
        elif n == 3:                     # of3(title, period, authors...)
            t = strings(args)
            title, periods, authors = t[0], [t[1]], t[2:]
        else:                            # of4(title, new String[]{periods}, authors...)
            mm = re.match(r'\s*"((?:[^"\\]|\\.)*)"\s*,\s*new String\[\]\{(.*?)\}\s*,?(.*)$', args)
            if not mm:
                bad.append((ln, line.strip()))
                continue
            title, periods, authors = mm.group(1), strings(mm.group(2)), strings(mm.group(3))
        entries[bid] = {
            "kind": kind, "kind_label": labels.get(kind, kind),
            "title": title, "periods": periods, "authors": authors,
            "raw_author": (comment or "").strip(),
        }
    return entries, bad

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    entries, bad = convert(sys.argv[1])
    json.dump(entries, open(sys.argv[2], "w", encoding="utf-8"), ensure_ascii=False, indent=0)
    print("entries:", len(entries), "unparsed:", len(bad))
    print(Counter(e["kind"] for e in entries.values()).most_common(5))
