// Asynchronous, non-blocking book-title reference linking for the read-only reader.
// Scans rendered text nodes for `《…》` citations, resolves unique titles in one
// backend call against already-loaded TitleIndexes (script-folded, current library first),
// and wraps only matched occurrences in `<a class="book-ref">` without touching source
// content or search highlight offsets.
import * as api from "./api.js";

const BOOK_TITLE_RE = /《([^《》\n。！？；：]{1,60})》/g;
const SKIP_SELECTOR = "a, .note, .nb-orig, .nb-cb, .fn, code, pre, script, style, .lb, .pb";
const MAX_CACHE_DOCS = 120;
const DOM_BATCH_SIZE = 80;

// LRU cache: key -> { version, pending, matches }
const docCache = new Map();
let titlesListenersAttached = false;
const refreshCallbacks = new Set();

function ensureTitlesListener() {
  if (titlesListenersAttached) return;
  titlesListenersAttached = true;
  api
    .onTitlesDone(() => {
      docCache.clear();
      for (const fn of refreshCallbacks) {
        try {
          fn();
        } catch {}
      }
    })
    .catch(() => {});
}

export function onBookRefsRefresh(fn) {
  ensureTitlesListener();
  refreshCallbacks.add(fn);
  return () => refreshCallbacks.delete(fn);
}

export function clearBookRefsCache() {
  docCache.clear();
}

function cacheGet(key) {
  const hit = docCache.get(key);
  if (!hit) return null;
  docCache.delete(key);
  docCache.set(key, hit);
  return hit;
}

function cacheSet(key, val) {
  if (docCache.has(key)) docCache.delete(key);
  docCache.set(key, val);
  if (docCache.size > MAX_CACHE_DOCS) {
    const oldest = docCache.keys().next().value;
    if (oldest !== undefined) docCache.delete(oldest);
  }
}

function nextIdle() {
  return new Promise((resolve) => {
    if (typeof requestIdleCallback === "function") {
      requestIdleCallback(() => resolve(), { timeout: 120 });
    } else {
      setTimeout(resolve, 0);
    }
  });
}

function nextFrame() {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

export function bookRefTooltip(raw, targets) {
  if (!targets?.length) return `标题快搜「${raw}」`;
  if (targets.length === 1) {
    const t = targets[0];
    const loc = t.is_current_lib ? "本库" : t.lib_title;
    return t.kind === "folder" && !t.path ? `在目录树中定位：${t.title}（${loc}）` : `跳转到：${t.title}（${loc}）`;
  }
  return `匹配到 ${targets.length} 项，点击选择`;
}

/**
 * Scan `rootEl` for `《…》` text nodes, resolve matched targets via backend
 * (with in-memory LRU caching), and decorate book titles in-place.
 * Matched titles link directly (or open a picker menu when multiple match);
 * unmatched titles get a subtle fuzzy link that opens title quick search when `allowFuzzy` is true.
 * Returns `{ matches: Map<string, Array>, pending: boolean }` or `null` if aborted.
 */
export async function annotateBookRefs(
  rootEl,
  { lib, path = null, bookId = null, stamp = null, simp = false, allowFuzzy = true, signal = null },
) {
  if (!rootEl || !lib) return null;
  await nextIdle();
  if (signal?.aborted || !rootEl.isConnected) return null;

  // 1. Collect candidate text nodes and unique book titles.
  const hits = []; // { node, ranges: [{ start, end, raw }] }
  const uniqueTitles = new Set();

  const walker = document.createTreeWalker(rootEl, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const v = node.nodeValue;
      if (!v || !v.includes("《")) return NodeFilter.FILTER_REJECT;
      const parent = node.parentElement;
      if (!parent || parent.closest(SKIP_SELECTOR)) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });

  let node;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue;
    BOOK_TITLE_RE.lastIndex = 0;
    let m;
    let ranges = null;
    while ((m = BOOK_TITLE_RE.exec(text))) {
      const raw = m[1].trim();
      if (!raw) continue;
      if (!ranges) ranges = [];
      ranges.push({ start: m.index, end: m.index + m[0].length, raw });
      uniqueTitles.add(raw);
    }
    if (ranges) hits.push({ node, ranges });
  }

  // Also include already-annotated book-ref spans so that refreshes after
  // `titles-done` keep their target lists and tooltips up to date.
  for (const a of rootEl.querySelectorAll("a.book-ref[data-book-ref]")) {
    const raw = a.dataset.bookRef?.trim();
    if (raw) uniqueTitles.add(raw);
  }

  if (uniqueTitles.size === 0) {
    return { matches: new Map(), pending: false };
  }

  // 2. Look up in LRU cache or query backend once for all unique titles.
  const cacheKey = `${lib}|${bookId ?? ""}|${path ?? ""}|${stamp ?? ""}|${simp ? "s" : "o"}`;
  let res = cacheGet(cacheKey);
  if (!res || res.pending) {
    try {
      res = await api.resolveBookRefs(lib, path, bookId, [...uniqueTitles]);
    } catch {
      return null;
    }
    if (signal?.aborted || !rootEl.isConnected) return null;
    cacheSet(cacheKey, res);
  }

  const matchObj = res?.matches || {};
  const matchMap = new Map(Object.entries(matchObj));
  if (matchMap.size === 0 && !allowFuzzy) {
    return { matches: matchMap, pending: Boolean(res?.pending) };
  }

  // 3. Wrap matched book titles (and unmatched ones as fuzzy QuickSearch fallbacks),
  // yielding between batches to keep UI smooth.
  let processed = 0;
  for (const { node: textNode, ranges } of hits) {
    if (signal?.aborted || !textNode.isConnected) return null;
    const toWrap = allowFuzzy ? ranges : ranges.filter((r) => matchMap.has(r.raw));
    if (!toWrap.length) continue;

    const text = textNode.nodeValue || "";
    const frag = document.createDocumentFragment();
    let cursor = 0;
    for (const r of toWrap) {
      if (r.start > cursor) {
        frag.appendChild(document.createTextNode(text.slice(cursor, r.start)));
      }
      const targets = matchMap.get(r.raw);
      const a = document.createElement("a");
      a.className = targets?.length ? "book-ref" : "book-ref book-ref-fuzzy";
      a.dataset.bookRef = r.raw;
      a.title = bookRefTooltip(r.raw, targets);
      a.textContent = text.slice(r.start, r.end);
      frag.appendChild(a);
      cursor = r.end;
    }
    if (cursor < text.length) {
      frag.appendChild(document.createTextNode(text.slice(cursor)));
    }
    textNode.parentNode?.replaceChild(frag, textNode);

    processed += 1;
    if (processed % DOM_BATCH_SIZE === 0) {
      await nextFrame();
      if (signal?.aborted || !rootEl.isConnected) return null;
    }
  }

  return { matches: matchMap, pending: Boolean(res?.pending) };
}
