#!/usr/bin/env python3
"""Reference extractor for CBETA volume XML (single-juan TEI P5 files).

Purpose: a small, readable oracle for the Rust implementation (M0b). It applies the
rules in docs/design-spec.md appendix A (derived from the old Java implementation):

  * body text = reading text only: <lem> yes, <rdg>/<note type=orig|mod|cf1..> no
  * <note place="inline"> (夹注) is kept, wrapped in full-width parentheses
  * <g ref="#CBxxxx"/> (missing glyph): charDecl normalized form > composition >
    Siddham glyph > charName
  * <space quantity="n"/> -> n spaces; milestone/pb/lb/unclear are skipped as text
  * source-file line breaks are layout only and are dropped
  * <lb ed="T"/> carries the original page/column/line; only lb whose ed equals the
    canon code is used

Output (for a volume file):
  text      : the punctuated reading text, no layout whitespace
  plain     : the same text without punctuation (what the index should see)
  line_map  : [(plain_offset, "0159b05"), ...] start offset of each original line,
              counted in `plain` characters

Usage:
  python tools/ref_extract.py samples/cbeta/T01n0004_001.xml            # summary
  python tools/ref_extract.py samples/cbeta/T01n0004_001.xml --golden DIR  # write golden files
"""
import json
import re
import sys
from lxml import etree

TEI = "http://www.tei-c.org/ns/1.0"
CB = "http://www.cbeta.org/ns/1.0"
XML = "http://www.w3.org/XML/1998/namespace"

def q(ns, name):
    return "{%s}%s" % (ns, name)

# punctuation that CBETA added (the original books have none)
PUNCT = set("，。、；：？！「」『』（）〔〕［］《》〈〉…—·．﹑﹔﹕﹖﹗“”‘’,.;:?!()[]<>")

def load_chardecl(root):
    decl = {}
    for ch in root.iter(q(TEI, "char")):
        cid = ch.get(q(XML, "id"))
        props = {}
        for p in ch.findall(q(TEI, "charProp")):
            name = (p.findtext(q(TEI, "localName")) or "").strip().lower()
            props[name] = (p.findtext(q(TEI, "value")) or "").strip()
        name = (ch.findtext(q(TEI, "charName")) or "").strip()
        text = (props.get("normalized form") or props.get("composition")
                or props.get("character in the siddham font") or name)
        decl[cid] = text
    return decl

class Extractor:
    def __init__(self, root, canon):
        self.canon = canon
        self.decl = load_chardecl(root)
        self.pieces = []         # (text, current_lb)
        self.cur_lb = None

    def emit(self, s):
        if s:
            self.pieces.append((s, self.cur_lb))

    def walk(self, el):
        tag = etree.QName(el).localname if isinstance(el.tag, str) else None
        if tag is None:                       # comment / PI
            return
        if tag in ("teiHeader",):
            return
        if tag == "lb":
            if el.get("ed") == self.canon:
                self.cur_lb = el.get("n")
            return                            # tail handled by the caller
        if tag in ("pb", "milestone", "unclear"):
            return
        if tag == "note":
            if el.get("place") == "inline":
                self.emit("（")
                self.children(el)
                self.emit("）")
            return                            # footnotes / apparatus notes: skipped
        if tag == "app":
            lem = el.find(q(TEI, "lem"))
            if lem is None:
                lem = el.find(q(TEI, "rdg"))
            if lem is not None:
                self.children(lem)
            return
        if tag == "rdg":
            return
        if tag == "g":
            ref = (el.get("ref") or "").lstrip("#")
            self.emit(self.decl.get(ref, "@Declare@" + ref))
            return
        if tag == "space":
            try:
                n = int(el.get("quantity") or 0)
            except ValueError:
                n = 0
            self.emit(" " * n)
            return
        if tag == "tt" and el.tag == q(CB, "tt"):
            if el.get("type") == "app":
                ts = el.findall(q(CB, "t"))
                pick = None
                for t in ts:
                    if t.get(q(XML, "lang")) == "zh-Hant":
                        pick = t
                        break
                if pick is None and ts:
                    pick = ts[0]
                if pick is not None:
                    self.children(pick)
            return
        self.children(el)

    def children(self, el):
        if el.text:
            self.emit(re.sub(r"[\r\n\t]", "", el.text))
        for c in el:
            self.walk(c)
            if c.tail:
                self.emit(re.sub(r"[\r\n\t]", "", c.tail))

def extract(path):
    tree = etree.parse(path)
    root = tree.getroot()
    canon = root.find(".//" + q(TEI, "idno") + "[@type='canon']")
    canon = canon.text.strip() if canon is not None else ""
    ex = Extractor(root, canon)
    body = root.find(".//" + q(TEI, "body"))
    ex.children(body)
    text = "".join(s for s, _ in ex.pieces)

    plain_chars = []
    line_map = []
    last_lb = None
    for s, lb in ex.pieces:
        for ch in s:
            if ch in PUNCT or ch.isspace():
                continue
            if lb != last_lb:
                line_map.append((len(plain_chars), lb))
                last_lb = lb
            plain_chars.append(ch)
    return text, "".join(plain_chars), line_map, ex

def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return
    path = sys.argv[1]
    text, plain, line_map, ex = extract(path)
    print("canon text chars :", len(text))
    print("plain chars      :", len(plain))
    print("lines in map     :", len(line_map))
    print("first lines      :", line_map[:5])
    print("text head        :", text[:60])
    if "--golden" in sys.argv:
        out = sys.argv[sys.argv.index("--golden") + 1]
        stem = path.rsplit("/", 1)[-1].rsplit(".", 1)[0]
        open("%s/%s.text.txt" % (out, stem), "w", encoding="utf-8").write(text + "\n")
        open("%s/%s.plain.txt" % (out, stem), "w", encoding="utf-8").write(plain + "\n")
        json.dump([{"offset": o, "lb": n} for o, n in line_map],
                  open("%s/%s.linemap.json" % (out, stem), "w", encoding="utf-8"),
                  ensure_ascii=False, indent=1)
        print("golden files written to", out)

if __name__ == "__main__":
    main()
