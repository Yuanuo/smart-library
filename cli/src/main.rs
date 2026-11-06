//! smart-library-cli: command line front end of Smart Library used to check the core library against a real bookcase.

use smart_library_core::bookcase::Bookcase;
use smart_library_core::catalog::Book;
use smart_library_core::nav::NavNode;
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
  smart-library check <bookcase.zip|dir>"
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
        _ => {
            usage();
            std::process::exit(2);
        }
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
