# samples

Real data seen while designing, kept small. The full `bookcase.zip` is NOT in the repo
(license + size): keep it outside, e.g. `C:\Devx\smartLib.d\bookcase.zip`.

| file | what it is |
| --- | --- |
| cbeta/index.xml | bookcase entry file |
| cbeta/catalog_head.txt | first 5 lines of `catalog.txt` (UTF-8, separator ` , `) |
| cbeta/bookdata_head.txt | first 5 lines of `bookdata.txt` (the real file is UTF-16LE with BOM) |
| cbeta/spine_head.txt | first 5 lines of `spine.txt` |
| cbeta/menu_nav.xhtml | list of the three trees (first link is not wrapped in `<li>`) |
| cbeta/simple_nav_head.xhtml, other_nav_fragment.xhtml | heads of `*_nav.xhtml` (truncated, not well formed) |
| cbeta/toc_T0004.xml, toc_T0262.xml | per-book toc files (`toc/T/T0004.xml`, `toc/T/T0262.xml`) |
| cbeta/longzang_nav_fragment.xhtml | fragment of the user-made `longzang_nav.xhtml`: a link node that also has children |
| cbeta/toc_X0714.xml | toc of a book whose juan 3 is split across two physical volumes (X39n0714_003 + X40n0714_003) |
| cbeta/T01n0004_001.xml | one complete volume XML (TEI P5, single juan) |
| cbeta/T01n0004_001.{text.txt,plain.txt,linemap.json} | golden output of `tools/ref_extract.py` for that volume |

The golden files come from the Python reference extractor and are an oracle for the Rust
port, not ground truth: where the rules are uncertain (see design-spec appendix A.6) the
extractor follows the old Java behaviour.
