#!/usr/bin/env python3
"""Differential test: Rust extraction vs tools/ref_extract.py on real volumes.

    smart-library volumes <bookcase.zip> XML/ --every 20 --out OUT
    python tools/diff_extract.py <bookcase.zip> OUT

For every `{stem}.plain.txt` written by the Rust CLI, the matching volume XML is read
from the zip, run through ref_extract.extract(), and text / plain / line map are
compared. Prints a summary and the first differences. Needs lxml.
"""
import io
import json
import os
import sys
import zipfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ref_extract  # noqa: E402


def first_diff(a, b):
    n = min(len(a), len(b))
    for i in range(n):
        if a[i] != b[i]:
            return i
    return n if len(a) != len(b) else -1


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    zpath, out = sys.argv[1], sys.argv[2]
    z = zipfile.ZipFile(zpath, metadata_encoding="gbk")
    by_stem = {}
    for n in z.namelist():
        if "/XML/" in n and n.endswith(".xml"):
            by_stem[n.rsplit("/", 1)[1][:-4]] = n
    stems = sorted(f[: -len(".plain.txt")] for f in os.listdir(out) if f.endswith(".plain.txt"))
    same = 0
    diffs = []
    for stem in stems:
        read = lambda ext: open(os.path.join(out, stem + ext), encoding="utf-8").read()
        rust_text, rust_plain = read(".text.txt")[:-1], read(".plain.txt")[:-1]
        rust_lines = [(d["offset"], d["lb"]) for d in json.loads(read(".linemap.json"))]
        text, plain, lines, _ = ref_extract.extract(io.BytesIO(z.read(by_stem[stem])))
        problems = []
        if text != rust_text:
            i = first_diff(text, rust_text)
            problems.append("text @%d py=%r rs=%r" % (i, text[i:i + 12], rust_text[i:i + 12]))
        if plain != rust_plain:
            i = first_diff(plain, rust_plain)
            problems.append("plain @%d py=%r rs=%r" % (i, plain[i:i + 12], rust_plain[i:i + 12]))
        if [(o, l) for o, l in lines] != rust_lines:
            problems.append("line map: py %d entries, rs %d" % (len(lines), len(rust_lines)))
        if problems:
            diffs.append((stem, problems))
        else:
            same += 1
    print("volumes compared: %d, identical: %d, different: %d" % (len(stems), same, len(diffs)))
    for stem, problems in diffs[:20]:
        print("  " + stem)
        for p in problems:
            print("      " + p)
    return 0 if not diffs else 1


if __name__ == "__main__":
    sys.exit(main())
