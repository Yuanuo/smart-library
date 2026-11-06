//! smart-library-cli: command line front end of Smart Library used to check the core library against a real bookcase.

use smart_library_core::bookcase::Bookcase;
use smart_library_core::catalog::Book;
use smart_library_core::nav::NavNode;
use smart_library_core::search::{locate, snippets, MatchMode, SearchIndex, SearchRequest};
use smart_library_core::source::{DirSource, Source, ZipSource};
use smart_library_core::toc::TocItem;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

fn usage() {
    eprintln!(
        "usage:
  smart-library info  <bookcase.zip|dir>
  smart-library tree  <bookcase.zip|dir> [tree id from `info`, e.g. simple] [--depth N]
  smart-library find  <bookcase.zip|dir> <keyword>
  smart-library book  <bookcase.zip|dir> <book id, e.g. T0262>
  smart-library check <bookcase.zip|dir>
  smart-library text  <bookcase.zip|dir> <book id> [juan] [--punct]
  smart-library text  <bookcase.zip|dir> <volume path, e.g. XML/T/T01/T01n0004_001.xml> [--punct]
  smart-library volumes <bookcase.zip|dir> [path prefix, e.g. XML/T/] [--out DIR] [--every N] [--keep-mulu]
  smart-library index <bookcase.zip|dir> [--index DIR]
  smart-library search <bookcase.zip|dir> <query> [--phrase] [--scope bulei/阿含部類] [--page N] [--limit N]
                       [--facet bulei/阿含部類] [--index DIR]

  The index directory defaults to app_data/repos/cbeta/index (it can be deleted and rebuilt).
  search: loose match by default (every bigram in the same volume; a space separates
  keywords); --phrase for an exact phrase (punctuation ignored)."
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
        std::process::exit(2);
    }
    let cmd = args[1].as_str();
    let bc = match open(&args[2]) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    match cmd {
        "info" => cmd_info(&bc),
        "tree" => {
            let tree_id = args
                .get(3)
                .filter(|a| !a.starts_with("--"))
                .map(|s| s.as_str())
                .unwrap_or("simple");
            let depth = args
                .iter()
                .position(|a| a == "--depth")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(2);
            cmd_tree(&bc, tree_id, depth);
        }
        "find" => match args.get(3) {
            Some(kw) => cmd_find(&bc, kw),
            None => {
                usage();
                std::process::exit(2);
            }
        },
        "book" => match args.get(3) {
            Some(id) => cmd_book(&bc, id),
            None => {
                usage();
                std::process::exit(2);
            }
        },
        "check" => cmd_check(&bc),
        "text" => match args.get(3) {
            Some(target) => {
                let juan = args.get(4).filter(|a| !a.starts_with("--")).map(|s| s.as_str());
                cmd_text(&bc, target, juan, args.iter().any(|a| a == "--punct"));
            }
            None => {
                usage();
                std::process::exit(2);
            }
        },
        "index" => {
            let dir = flag_value(&args, "--index").unwrap_or(DEFAULT_INDEX);
            cmd_index(&bc, Path::new(dir));
        }
        "search" => match args.get(3).filter(|a| !a.starts_with("--")) {
            Some(q) => {
                let dir = flag_value(&args, "--index").unwrap_or(DEFAULT_INDEX);
                let mut req = SearchRequest::new(q);
                if args.iter().any(|a| a == "--phrase") {
                    req.mode = MatchMode::Phrase;
                }
                req.limit = flag_value(&args, "--limit").and_then(|v| v.parse().ok()).unwrap_or(10);
                let page: usize = flag_value(&args, "--page").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
                req.offset = (page - 1) * req.limit;
                req.scope = flag_value(&args, "--scope").map(|s| s.to_string());
                req.facet_drill = flag_value(&args, "--facet").map(|s| vec![s.to_string()]).unwrap_or_default();
                cmd_search(&bc, Path::new(dir), &req);
            }
            None => {
                usage();
                std::process::exit(2);
            }
        },
        "volumes" => {
            let prefix = args.get(3).filter(|a| !a.starts_with("--")).map(|s| s.as_str()).unwrap_or("XML/");
            let flag = |name: &str| args.iter().position(|a| a == name).and_then(|p| args.get(p + 1));
            let every = flag("--every").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
            cmd_volumes(&bc, prefix, flag("--out").map(|s| s.as_str()), every, args.iter().any(|a| a == "--keep-mulu"));
        }
        _ => {
            usage();
            std::process::exit(2);
        }
    }
}

const DEFAULT_INDEX: &str = "app_data/repos/cbeta/index";

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|p| args.get(p + 1)).map(|s| s.as_str())
}

fn cmd_index(bc: &Bookcase, dir: &Path) {
    println!("building index for {} ({}) in {}", bc.data_version(), bc.version, dir.display());
    let res = SearchIndex::build(bc, dir, |done, total| {
        eprint!("\r  {}/{} volumes", done, total);
    });
    eprintln!();
    match res {
        Ok(st) => {
            let size: u64 = std::fs::read_dir(dir)
                .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
                .unwrap_or(0);
            println!(
                "indexed {} volumes in {:.1}s, {} skipped, index size {:.0} MB",
                st.volumes,
                st.seconds,
                st.skipped.len(),
                size as f64 / 1e6
            );
            for e in st.skipped.iter().take(10) {
                println!("  skipped {}", e);
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_search(bc: &Bookcase, dir: &Path, req: &SearchRequest) {
    if let Some(v) = SearchIndex::data_version(dir) {
        if v != bc.data_version() {
            eprintln!("warning: index built for {}, bookcase is {}: rebuild the index", v, bc.data_version());
        }
    }
    let idx = match SearchIndex::open(dir) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    let t0 = std::time::Instant::now();
    let r = match idx.search(req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    let t_search = t0.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{} volumes in {} books  ({}, {:.1} ms)",
        r.total_volumes,
        r.total_books,
        if req.mode == MatchMode::Phrase { "phrase" } else { "loose" },
        t_search
    );
    for f in &r.facets {
        let shown: Vec<String> = f.values.iter().take(8).map(|(n, c)| format!("{} {}", n, c)).collect();
        if !shown.is_empty() {
            println!(
                "  {:<6} {}{}",
                smart_library_core::search::facet_label(&f.root),
                shown.join(" · "),
                if f.values.len() > 8 { format!(" · …({})", f.values.len()) } else { String::new() }
            );
        }
    }
    let t1 = std::time::Instant::now();
    for (i, h) in r.hits.iter().enumerate() {
        println!();
        println!("{:>3}. {} {} 卷{}  [{}]  score {:.2}", req.offset + i + 1, h.book, h.title, h.juan, h.path, h.score);
        match bc.volume_text(&h.path) {
            Ok(vt) => {
                let occ = locate(&vt, &req.query, req.mode);
                for sn in snippets(&vt, &occ, 12, 3) {
                    let s: String = sn
                        .parts
                        .iter()
                        .map(|(t, m)| if *m { format!("【{}】", t) } else { t.clone() })
                        .collect();
                    println!("     {} 第{}字  {}", sn.lb, sn.pos, s);
                }
                println!("     ({} occurrence(s) in this volume)", occ.len());
            }
            Err(e) => println!("     (cannot locate: {})", e),
        }
    }
    if !r.hits.is_empty() {
        println!();
        println!("located {} volume(s) in {:.1} ms", r.hits.len(), t1.elapsed().as_secs_f64() * 1000.0);
    }
}

fn open(path: &str) -> Result<Bookcase, String> {
    let p = Path::new(path);
    let source: Box<dyn Source> = if p.is_dir() {
        Box::new(DirSource::open(p)?)
    } else {
        Box::new(ZipSource::open(p)?)
    };
    Bookcase::open(source)
}

fn cmd_info(bc: &Bookcase) {
    println!("version      : {}", bc.version);
    println!("date         : {}", bc.date);
    println!("data version : {}", bc.data_version());
    println!("files        : {}", bc.source.list().len());
    println!("books        : {} ({} catalog lines skipped)", bc.catalog.books.len(), bc.catalog.skipped);
    println!("canons       : {}", bc.tripitakas.len());
    for t in &bc.tripitakas {
        println!("  {:<3} {} {}  ({})", t.id, t.tag, t.name, t.name_en);
    }
    println!("trees        : {}", bc.trees.len());
    for t in &bc.trees {
        println!("  {:<8} {}  [{}]", t.id, t.title, t.file);
    }
}

fn cmd_tree(bc: &Bookcase, tree_id: &str, max_depth: usize) {
    let doc = match bc.nav(tree_id) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    for node in &doc.items {
        print_node(bc, node, 0, max_depth);
    }
}

fn print_node(bc: &Bookcase, node: &NavNode, depth: usize, max_depth: usize) {
    let pad = "  ".repeat(depth);
    match node {
        NavNode::Folder { title, children } => {
            println!("{}{}  ({} children)", pad, title, children.len());
            if depth < max_depth {
                for c in children {
                    print_node(bc, c, depth + 1, max_depth);
                }
            }
        }
        NavNode::Leaf { href, text, children, .. } => {
            let more = if children.is_empty() { String::new() } else { format!("  ({} children)", children.len()) };
            match bc.resolve_leaf(href, text) {
                Some(e) => println!("{}- {} {}{}{}", pad, e.book_id, e.title, if e.cloned { "  [part]" } else { "" }, more),
                None => println!("{}- ? {}  [{}]{}", pad, text, href, more),
            }
            if depth < max_depth {
                for c in children {
                    print_node(bc, c, depth + 1, max_depth);
                }
            }
        }
    }
}

fn cmd_find(bc: &Bookcase, kw: &str) {
    let found = bc.find_books(kw);
    for b in found.iter().take(50) {
        println!("{:<8} {}  [{}]  {}", b.id, b.title, b.catalog, b.author_info);
    }
    println!("{} book(s) found{}", found.len(), if found.len() > 50 { " (first 50 shown)" } else { "" });
}

fn cmd_book(bc: &Bookcase, id: &str) {
    let book = match bc.book(id) {
        Some(b) => b,
        None => {
            eprintln!("unknown book id: {}", id);
            std::process::exit(1);
        }
    };
    println!("{}  {}", book.id, book.title);
    println!("catalog : {}", book.catalog);
    println!("author  : {}", book.author_info);
    println!("toc file: {}", book.toc_path());

    let vols = bc.volumes(id);
    let missing = vols.iter().filter(|v| !v.exists).count();
    println!("volumes : {} ({} missing in the source)", vols.len(), missing);
    for v in &vols {
        println!(
            "  {:>3} {:<6} {}{}{}",
            v.idx,
            v.label,
            v.path,
            if v.from_toc { "" } else { "  [from catalog.txt]" },
            if v.exists { "" } else { "  [MISSING]" }
        );
    }
    match bc.outline(id) {
        Some(toc) => {
            println!("outline : {} top-level chapter(s)", toc.chapters.len());
            print_toc(&toc.chapters, 1);
        }
        None => println!("outline : (no toc file)"),
    }
}

fn print_toc(items: &[TocItem], depth: usize) {
    for it in items {
        let file = it.link.file.rsplit('/').next().unwrap_or("");
        println!(
            "{}{}  [{}#{}]",
            "  ".repeat(depth),
            it.link.text,
            file,
            it.link.anchor.as_deref().unwrap_or("")
        );
        print_toc(&it.children, depth + 1);
    }
}

/// Sanity checks over the whole bookcase: paste the output back to find hidden cases.
fn cmd_check(bc: &Bookcase) {
    println!("== navigation trees");
    for t in &bc.trees {
        let doc = match bc.nav(&t.id) {
            Ok(d) => d,
            Err(e) => {
                println!("{}: error: {}", t.id, e);
                continue;
            }
        };
        let leaves = doc.leaves();
        let mut resolved = 0usize;
        let mut cloned = 0usize;
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut unresolved: Vec<(String, String)> = Vec::new();
        for (href, text) in &leaves {
            match bc.resolve_leaf(href, text) {
                Some(e) => {
                    resolved += 1;
                    if e.cloned {
                        cloned += 1;
                    }
                }
                None => {
                    let kind = href.split('/').next().unwrap_or("").to_string();
                    *kinds.entry(kind).or_insert(0) += 1;
                    unresolved.push((text.to_string(), href.to_string()));
                }
            }
        }
        println!(
            "{} ({}): {} leaves, {} resolved ({} parts), {} unresolved",
            t.id,
            t.file,
            leaves.len(),
            resolved,
            cloned,
            unresolved.len()
        );
        for (k, n) in &kinds {
            println!("    unresolved, href starts with `{}`: {}", k, n);
        }
        for (text, href) in unresolved.iter().take(10) {
            println!("    ? {}  [{}]", text, href);
        }
    }

    println!("== books");
    let mut no_toc: Vec<&Book> = Vec::new();
    let mut no_vol: Vec<&Book> = Vec::new();
    let mut missing: Vec<&Book> = Vec::new();
    let mut referenced: HashSet<String> = HashSet::new();
    for id in &bc.catalog.order {
        let book = match bc.book(id) {
            Some(b) => b,
            None => continue,
        };
        if bc.outline(id).is_none() {
            no_toc.push(book);
        }
        let vols = bc.volumes(id);
        if vols.is_empty() {
            no_vol.push(book);
        }
        if vols.iter().any(|v| !v.exists) {
            missing.push(book);
        }
        for v in &vols {
            referenced.insert(v.path.clone());
        }
    }
    println!("books: {}", bc.catalog.order.len());
    show("without a toc file", &no_toc);
    show("without any volume file", &no_vol);
    show("with volumes listed but missing", &missing);

    let mut orphans: Vec<String> = bc
        .source
        .list()
        .into_iter()
        .filter(|p| p.starts_with("XML/") && p.ends_with(".xml") && !referenced.contains(p))
        .collect();
    orphans.sort();
    // canons that bookdata.txt does not register (LC, Y in 2025.Q1) are not
    // included by design: report them as ignored, apart from real orphans
    let registered: HashSet<&str> = bc.tripitakas.iter().map(|t| t.id.as_str()).collect();
    let (orphans, unregistered): (Vec<String>, Vec<String>) = orphans
        .into_iter()
        .partition(|p| registered.contains(p.split('/').nth(1).unwrap_or("")));
    let mut ignored: std::collections::BTreeMap<&str, usize> = Default::default();
    for p in &unregistered {
        *ignored.entry(p.split('/').nth(1).unwrap_or("?")).or_default() += 1;
    }
    println!(
        "volume files of unregistered canons (ignored): {}  {:?}",
        unregistered.len(),
        ignored
    );
    println!("volume files not referenced by any book: {}", orphans.len());
    // group by canon folder, then by work (file name before the last `_`)
    let mut by_canon: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<&str>>> =
        Default::default();
    for p in &orphans {
        let canon = p.split('/').nth(1).unwrap_or("?").to_string();
        let file = p.rsplit('/').next().unwrap_or(p);
        let work = file.rsplit_once('_').map(|(w, _)| w).unwrap_or(file).to_string();
        by_canon.entry(canon).or_default().entry(work).or_default().push(p);
    }
    for (canon, works) in &by_canon {
        let n: usize = works.values().map(|v| v.len()).sum();
        println!("  {}: {} files in {} works", canon, n, works.len());
        for (work, files) in works.iter().take(8) {
            let first = files[0].rsplit('/').next().unwrap_or(files[0]);
            println!("      {} ({} files, e.g. {})", work, files.len(), first);
        }
        if works.len() > 8 {
            println!("      ... {} more works", works.len() - 8);
        }
    }
}

fn show(label: &str, books: &[&Book]) {
    println!("{}: {}", label, books.len());
    for b in books.iter().take(10) {
        println!("    {} {}", b.id, b.title);
    }
}

/// Print one volume, one original line per output line: `lb  text`.
fn cmd_text(bc: &Bookcase, target: &str, juan: Option<&str>, punct: bool) {
    let path = if target.starts_with("XML/") {
        target.to_string()
    } else {
        let vols = bc.volumes(target);
        let idx: u16 = juan.and_then(|j| j.parse().ok()).unwrap_or(1);
        match vols.iter().find(|v| v.idx == idx && v.exists) {
            Some(v) => v.path.clone(),
            None => {
                eprintln!("{}: no volume {} (see `book {}`)", target, idx, target);
                std::process::exit(1);
            }
        }
    };
    let v = match bc.volume_text(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    println!("{}  canon {}, {} chars, {} plain chars, {} lines", path, v.canon, v.text.chars().count(), v.plain.chars().count(), v.lines.len());
    if punct {
        println!("{}", v.text);
        return;
    }
    let plain: Vec<char> = v.plain.chars().collect();
    for (i, line) in v.lines.iter().enumerate() {
        let end = v.lines.get(i + 1).map(|l| l.offset).unwrap_or(plain.len());
        let s: String = plain[line.offset..end].iter().collect();
        println!("{:<8} {}", line.lb.as_deref().unwrap_or("-"), s);
    }
}

/// Extract many volumes: report failures and timing; with `--out`, write
/// `{stem}.text.txt`, `{stem}.plain.txt`, `{stem}.linemap.json` in the same
/// format as `tools/ref_extract.py --golden` for `tools/diff_extract.py`.
fn cmd_volumes(bc: &Bookcase, prefix: &str, out: Option<&str>, every: usize, keep_mulu: bool) {
    let mut paths: Vec<String> = bc
        .source
        .list()
        .into_iter()
        .filter(|p| p.starts_with(prefix) && p.ends_with(".xml") && p.starts_with("XML/"))
        .collect();
    paths.sort();
    let paths: Vec<String> = paths.into_iter().step_by(every).collect();
    if let Some(dir) = out {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("error: {}: {}", dir, e);
            std::process::exit(1);
        }
    }
    let start = std::time::Instant::now();
    let (mut ok, mut chars, mut no_lines) = (0usize, 0usize, Vec::new());
    let mut failed: Vec<String> = Vec::new();
    let (mut anchors_ok, mut anchors_bad) = (0usize, Vec::<String>::new());
    for p in &paths {
        let res = match (keep_mulu, bc.source.read(p)) {
            (true, Some(bytes)) => smart_library_core::volume::extract_volume_with(
                &smart_library_core::text::decode_auto(&bytes),
                false,
            ),
            _ => bc.volume_text(p),
        };
        match res {
            Ok(v) => {
                ok += 1;
                // `p` ids end with the line (7 chars) and the 1-based position in it
                for (id, off) in &v.anchors {
                    let n = id.len();
                    if n < 9 || !id.is_char_boundary(n - 9) {
                        continue;
                    }
                    let (want_lb, want_pos) = (&id[n - 9..n - 2], id[n - 2..].parse::<usize>().ok());
                    let got = v.locate(*off).map(|(l, pos)| (l.lb.clone().unwrap_or_default(), pos));
                    // a paragraph that starts right at a line end is reported on the next line
                    let at_prev_end = |lb: &str, pos: usize| {
                        pos == 1
                            && v.lines.iter().position(|l| l.lb.as_deref() == Some(lb)).map_or(false, |i| {
                                i > 0
                                    && v.lines[i - 1].lb.as_deref() == Some(want_lb)
                                    && Some(v.lines[i].offset - v.lines[i - 1].offset + 1) == want_pos
                            })
                    };
                    match (got, want_pos) {
                        (Some((lb, pos)), Some(wp)) if lb == want_lb && pos == wp => anchors_ok += 1,
                        (Some((lb, pos)), _) if at_prev_end(&lb, pos) => anchors_ok += 1,
                        (got, _) => anchors_bad.push(format!("{} {} -> {:?}", p, id, got)),
                    }
                }
                chars += v.plain.chars().count();
                if v.lines.iter().all(|l| l.lb.is_none()) {
                    no_lines.push(p.clone());
                }
                if let Some(dir) = out {
                    let stem = p.rsplit('/').next().unwrap_or(p).trim_end_matches(".xml");
                    let base = Path::new(dir).join(stem);
                    let mut json = String::from("[");
                    for (i, l) in v.lines.iter().enumerate() {
                        let lb = match &l.lb {
                            Some(s) => format!("\"{}\"", s),
                            None => "null".to_string(),
                        };
                        json.push_str(if i == 0 { "
" } else { ",
" });
                        json.push_str(&format!(" {{
  \"offset\": {},
  \"lb\": {}
 }}", l.offset, lb));
                    }
                    json.push_str("
]");
                    let w = |ext: &str, s: &str| std::fs::write(base.with_extension(ext), s);
                    if let Err(e) = w("text.txt", &format!("{}
", v.text))
                        .and(w("plain.txt", &format!("{}
", v.plain)))
                        .and(w("linemap.json", &json))
                    {
                        eprintln!("error writing {}: {}", base.display(), e);
                    }
                }
            }
            Err(e) => failed.push(e),
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!("volumes: {} (every {}), ok {}, failed {}", paths.len(), every, ok, failed.len());
    println!("plain chars: {}  time: {:.1}s ({:.2} ms/volume)", chars, secs, secs * 1000.0 / paths.len().max(1) as f64);
    for e in failed.iter().take(20) {
        println!("  FAILED {}", e);
    }
    println!("paragraph anchors matching their line/position: {} ok, {} not", anchors_ok, anchors_bad.len());
    let mut bad_by_canon: BTreeMap<&str, usize> = BTreeMap::new();
    for a in &anchors_bad {
        *bad_by_canon.entry(a.split('/').nth(1).unwrap_or("?")).or_default() += 1;
    }
    println!("    by canon: {:?}", bad_by_canon);
    for a in anchors_bad.iter().step_by((anchors_bad.len() / 15).max(1)).take(15) {
        println!("    {}", a);
    }
    println!("volumes without any lb of their canon: {}", no_lines.len());
    for p in no_lines.iter().take(10) {
        println!("    {}", p);
    }
}
