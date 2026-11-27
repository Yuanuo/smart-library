// In-document and cross-sibling-document 科判 (kepan) bidirectional linking for
// the read-only reader.
// Parses rendered `p.kepan` paragraphs into a level-scoped hierarchy using
// Heavenly Stems / Earthly Branches (甲..癸 = 1..10, 子..亥 = 11..22),
// wraps matched sub-items inside `p.kepan-split` as jump links to child
// paragraphs, and appends a copy-safe back-to-parent button on matched children.
import * as api from "./api.js";

const MAX_SIBLING_CACHE = 40;
const siblingCache = new Map(); // cacheKey -> Promise<SiblingKepansDto[]>

function siblingCacheGet(key) {
  const hit = siblingCache.get(key);
  if (!hit) return null;
  siblingCache.delete(key);
  siblingCache.set(key, hit);
  return hit;
}

function siblingCacheSet(key, val) {
  if (siblingCache.has(key)) siblingCache.delete(key);
  siblingCache.set(key, val);
  if (siblingCache.size > MAX_SIBLING_CACHE) {
    const oldest = siblingCache.keys().next().value;
    if (oldest !== undefined) siblingCache.delete(oldest);
  }
}

const GANZHI = "甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉戌亥";
const CN_NUM_RE = /^([一二三四五六七八九十百千零〇廿卅0-9]+)/;
const GANZHI_NUM_RE = /^([甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉戌亥])([一二三四五六七八九十百千零〇廿卅0-9]+)/;
const FEN_COLON_RE = /分([一二三四五六七八九十百千零〇廿卅0-9]+)[：:]/;
const SKIP_TEXT_SELECTOR = ".lb, .pb, .fn, .note, .nb-orig, .nb-cb, .kepan-back";

/** Parse simple Chinese or Arabic numerals (1..999) for ordering kepan children. */
export function parseKepanNum(str) {
  if (!str) return 0;
  if (/^\d+$/.test(str)) return Number(str);
  const digits = { 零: 0, 〇: 0, 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9 };
  if (str === "廿") return 20;
  if (str === "卅") return 30;
  let total = 0;
  let cur = 0;
  for (const ch of str) {
    if (ch in digits) {
      cur = digits[ch];
    } else if (ch === "十") {
      total += (cur || 1) * 10;
      cur = 0;
    } else if (ch === "廿") {
      total += 20;
      cur = 0;
    } else if (ch === "卅") {
      total += 30;
      cur = 0;
    } else if (ch === "百") {
      total += (cur || 1) * 100;
      cur = 0;
    } else if (ch === "千") {
      total += (cur || 1) * 1000;
      cur = 0;
    }
  }
  return total + cur;
}

/** Collect visible text segments of an element, skipping markers/footnotes/back buttons. */
function collectTextSegments(root) {
  const segs = [];
  let offset = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let node;
  while ((node = walker.nextNode())) {
    if (node.parentElement?.closest(SKIP_TEXT_SELECTOR)) continue;
    const text = node.nodeValue || "";
    if (!text) continue;
    segs.push({ node, start: offset, end: offset + text.length });
    offset += text.length;
  }
  return { segs, fullText: segs.map((s) => s.node.nodeValue || "").join("") };
}

function normalizeTitle(s) {
  return (s || "")
    .replace(/[（(][^）)]*[）)]/g, "")
    .replace(/[\s，。！？；：、,.!?;:“”‘’「」『』—\-]+/g, "")
    .trim();
}

function titlesCompatible(a, b) {
  const na = normalizeTitle(a);
  const nb = normalizeTitle(b);
  if (!na || !nb) return false;
  return na === nb || na.includes(nb) || nb.includes(na);
}

/**
 * Parse a kepan paragraph's header info and (if split) its enumerated child items
 * with exact character offsets in `fullText`.
 */
export function parseKepanText(fullText, isSplit) {
  const leadWs = fullText.length - fullText.trimStart().length;
  const trimmed = fullText.trim();
  const gzMatch = trimmed.match(GANZHI_NUM_RE);

  let level = 0;
  let num = 0;
  let label = "";
  let title = "";

  if (gzMatch) {
    const gz = gzMatch[1];
    const numStr = gzMatch[2];
    level = GANZHI.indexOf(gz) + 1;
    num = parseKepanNum(numStr);
    label = gz + numStr;
    const rest = trimmed.slice(label.length);
    if (isSplit) {
      const paren = rest.match(/^[（(]([^）)]+)[）)]/);
      if (paren) {
        title = paren[1].trim();
      } else {
        const fenIdx = rest.search(FEN_COLON_RE);
        title = (fenIdx >= 0 ? rest.slice(0, fenIdx) : rest).replace(/^[、\s]+/, "").trim();
      }
    } else {
      title = rest.replace(/^[、\s]+/, "").replace(/[：:]\s*$/, "").trim();
    }
  } else if (isSplit) {
    const fenIdx = trimmed.search(FEN_COLON_RE);
    label = (fenIdx > 0 ? trimmed.slice(0, fenIdx) : trimmed.slice(0, 6)).trim();
    title = label;
  }

  let items = [];
  if (isSplit) {
    const fenMatch = trimmed.match(FEN_COLON_RE);
    if (fenMatch && fenMatch.index !== undefined) {
      const expectedCount = parseKepanNum(fenMatch[1]);
      const bodyRelStart = fenMatch.index + fenMatch[0].length;
      const bodyStart = leadWs + bodyRelStart;
      const rawBody = fullText.slice(bodyStart);
      const bodyTrimEnd = rawBody.replace(/[。；;\s]+$/, "");
      items = splitSubItems(bodyTrimEnd, bodyStart, expectedCount, level);
    }
  }

  return { level, num, label, title, items };
}

function splitSubItems(body, baseOffset, expectedCount, parentLevel) {
  // Primary split by semicolon `；` / `;`
  let parts = splitWithOffsets(body, /[；;]/g, baseOffset);

  // Fallback: if no semicolons but expectedCount >= 2, split on `、` when there are no
  // stem/branch markers inside (e.g. `分二：真实宣说、运用其比喻`).
  if (parts.length === 1 && expectedCount >= 2 && !GANZHI_NUM_RE.test(parts[0].text)) {
    const byDun = splitWithOffsets(body, /、/g, baseOffset);
    if (byDun.length === expectedCount) {
      parts = byDun;
    }
  }

  return parts.map((p, idx) => {
    const t = p.text;
    const gz = t.match(GANZHI_NUM_RE);
    if (gz) {
      const childLevel = GANZHI.indexOf(gz[1]) + 1;
      const childNum = parseKepanNum(gz[2]);
      const childTitle = t.slice(gz[0].length).replace(/^[、\s]+/, "").trim();
      return { ...p, childLevel, childNum, childTitle, targetLb: null, targetTitle: "" };
    }
    const cn = t.match(/^([一二三四五六七八九十百千零〇廿卅0-9]+)、\s*(.*)$/);
    if (cn) {
      return {
        ...p,
        childLevel: parentLevel + 1,
        childNum: parseKepanNum(cn[1]),
        childTitle: cn[2].trim(),
        targetLb: null,
        targetTitle: "",
      };
    }
    return {
      ...p,
      childLevel: parentLevel + 1,
      childNum: idx + 1,
      childTitle: t,
      targetLb: null,
      targetTitle: "",
    };
  });
}

function splitWithOffsets(str, sepRe, baseOffset) {
  const out = [];
  let last = 0;
  let m;
  sepRe.lastIndex = 0;
  while ((m = sepRe.exec(str)) !== null) {
    pushTrimmed(out, str, last, m.index, baseOffset);
    last = m.index + m[0].length;
  }
  pushTrimmed(out, str, last, str.length, baseOffset);
  return out;
}

function pushTrimmed(out, str, from, to, baseOffset) {
  let s = from;
  let e = to;
  while (s < e && /\s/.test(str[s])) s += 1;
  while (e > s && /[\s。；;]/.test(str[e - 1])) e -= 1;
  if (e > s) {
    out.push({ text: str.slice(s, e), start: baseOffset + s, end: baseOffset + e });
  }
}

/** Wrap a `[start, end)` character range across text segments inside a kepan-split. */
function wrapRange(segs, start, end, lb, tooltip, targetPath = null) {
  // Find segments intersecting [start, end). If the range is inside a single text
  // node (the common case), wrap directly; if split across inline nodes (e.g. search
  // `<mark>`), wrap each piece so highlights stay intact.
  const hits = segs.filter((s) => s.end > start && s.start < end);
  if (!hits.length) return;
  for (let i = hits.length - 1; i >= 0; i -= 1) {
    const seg = hits[i];
    if (seg.node.parentElement?.closest("a")) continue;
    const localStart = Math.max(0, start - seg.start);
    const localEnd = Math.min(seg.end - seg.start, end - seg.start);
    if (localEnd <= localStart) continue;
    const val = seg.node.nodeValue || "";
    const before = val.slice(0, localStart);
    const mid = val.slice(localStart, localEnd);
    const after = val.slice(localEnd);
    const frag = document.createDocumentFragment();
    if (before) frag.appendChild(document.createTextNode(before));
    const a = document.createElement("a");
    a.className = targetPath ? "kepan-link kepan-cross" : "kepan-link";
    a.href = `#${lb}`;
    a.dataset.kepanLb = lb;
    if (targetPath) a.dataset.kepanPath = targetPath;
    if (tooltip) a.title = tooltip;
    a.textContent = mid;
    frag.appendChild(a);
    if (after) frag.appendChild(document.createTextNode(after));
    seg.node.parentNode?.replaceChild(frag, seg.node);
  }
}

/**
 * Split `textNode` at `localOffset` and bubble the split up to a direct child of
 * `root`, returning the direct child node that begins at that split point.
 */
function splitTreeAt(root, textNode, localOffset) {
  let node = textNode;
  if (localOffset > 0 && localOffset < (node.nodeValue || "").length) {
    node = node.splitText(localOffset);
  }
  while (node.parentNode && node.parentNode !== root) {
    const parent = node.parentNode;
    if (node.previousSibling) {
      const right = parent.cloneNode(false);
      if (right.nodeType === Node.ELEMENT_NODE) right.removeAttribute("id");
      let cur = node;
      while (cur) {
        const next = cur.nextSibling;
        right.appendChild(cur);
        cur = next;
      }
      parent.parentNode?.insertBefore(right, parent.nextSibling);
      node = right;
    } else {
      node = parent;
    }
  }
  return node;
}

/**
 * Undo multiline layout wrappers injected into `p.kepan-split` inside `docEl`.
 */
export function clearKepanLayout(docEl) {
  if (!docEl) return;
  for (const el of docEl.querySelectorAll("p.kepan.kepan-split")) {
    const spans = el.querySelectorAll(":scope > span.kepan-split-head, :scope > span.kepan-split-item");
    if (!spans.length) continue;
    for (const span of spans) {
      while (span.firstChild) el.insertBefore(span.firstChild, span);
      span.remove();
    }
    el.normalize();
  }
}

/**
 * Format `p.kepan.kepan-split` paragraphs so the header (`……分三：`) stays on
 * the first line and each enumerated sub-item (`丁一、……；`) renders on its own
 * indented line, preserving original text, punctuation, footnotes, and `<mark>` highlights.
 */
export function formatKepanSplits(docEl) {
  if (!docEl) return;
  clearKepanLayout(docEl);

  for (const el of docEl.querySelectorAll("p.kepan.kepan-split")) {
    const { fullText } = collectTextSegments(el);
    const parsed = parseKepanText(fullText, true);
    if (!parsed.items || parsed.items.length < 2) continue;

    // Split DOM right-to-left at each sub-item's start offset so earlier offsets stay valid.
    const starts = new Array(parsed.items.length);
    let ok = true;
    for (let i = parsed.items.length - 1; i >= 0; i -= 1) {
      const offset = parsed.items[i].start;
      const { segs } = collectTextSegments(el);
      const seg = segs.find((s) => offset >= s.start && offset < s.end);
      if (!seg) {
        ok = false;
        break;
      }
      starts[i] = splitTreeAt(el, seg.node, offset - seg.start);
    }
    if (!ok || starts.some((n) => !n || n.parentNode !== el)) {
      el.normalize();
      continue;
    }

    // Wrap header nodes (excluding leading `.lb`/`.pb` markers) in `.kepan-split-head`.
    const headSpan = document.createElement("span");
    headSpan.className = "kepan-split-head";
    let cur = el.firstChild;
    while (cur && cur !== starts[0] && cur.nodeType === Node.ELEMENT_NODE && cur.matches(".lb, .pb")) {
      cur = cur.nextSibling;
    }
    if (cur && cur !== starts[0]) {
      el.insertBefore(headSpan, cur);
      while (cur && cur !== starts[0]) {
        const next = cur.nextSibling;
        headSpan.appendChild(cur);
        cur = next;
      }
    }

    // Wrap each sub-item (including its trailing delimiter/punctuation) in `.kepan-split-item`.
    for (let i = 0; i < starts.length; i += 1) {
      const from = starts[i];
      const until = i + 1 < starts.length ? starts[i + 1] : null;
      const itemSpan = document.createElement("span");
      itemSpan.className = "kepan-split-item";
      el.insertBefore(itemSpan, from);
      let node = from;
      while (node && node !== until) {
        const next = node.nextSibling;
        itemSpan.appendChild(node);
        node = next;
      }
    }
  }
}

/**
 * Clean any previously injected kepan link wrappers or back buttons inside `docEl`.
 */
export function clearKepanLinks(docEl) {
  if (!docEl) return;
  for (const btn of docEl.querySelectorAll("button.kepan-back")) {
    btn.remove();
  }
  for (const a of docEl.querySelectorAll("a.kepan-link")) {
    const parent = a.parentNode;
    if (!parent) continue;
    while (a.firstChild) parent.insertBefore(a.firstChild, a);
    a.remove();
  }
}

/**
 * Build in-document bidirectional links between `p.kepan-split` and child `p.kepan`
 * paragraphs inside `docEl`. Returns summary info useful for future cross-doc resolution.
 */
export function annotateKepanLinks(docEl) {
  if (!docEl) return null;
  clearKepanLinks(docEl);

  const els = [...docEl.querySelectorAll("p.kepan")];
  if (!els.length) return null;

  const nodes = els.map((el) => {
    const lb = el.querySelector(".lb[data-lb]")?.dataset.lb || "";
    const isSplit = el.classList.contains("kepan-split");
    const { segs, fullText } = collectTextSegments(el);
    const parsed = parseKepanText(fullText, isSplit);
    return {
      el,
      lb,
      isSplit,
      segs,
      fullText,
      ...parsed,
      parentLb: null,
      parentLabel: "",
      parentTitle: "",
    };
  });

  // Level-scoped stack to match parent `kepan-split` <-> child `kepan`.
  const stack = [];
  for (const cur of nodes) {
    while (stack.length && stack[stack.length - 1].level >= cur.level) {
      stack.pop();
    }
    const parent = stack.length ? stack[stack.length - 1] : null;
    if (parent && parent.isSplit && parent.items.length && cur.level > 0) {
      const expectedLevel = parent.level + 1;
      if (cur.level === expectedLevel) {
        // 1. Exact childNum + compatible title
        let item = parent.items.find(
          (it) => !it.targetLb && it.childNum === cur.num && titlesCompatible(it.childTitle, cur.title),
        );
        // 2. Exact childNum
        if (!item) {
          item = parent.items.find((it) => !it.targetLb && it.childNum === cur.num);
        }
        // 3. Compatible title fallback
        if (!item) {
          item = parent.items.find((it) => !it.targetLb && titlesCompatible(it.childTitle, cur.title));
        }
        if (item && cur.lb && parent.lb) {
          item.targetLb = cur.lb;
          item.targetTitle = cur.label + (cur.title ? `、${cur.title}` : "");
          cur.parentLb = parent.lb;
          cur.parentLabel = parent.label;
          cur.parentTitle = parent.title ? `${parent.label}（${parent.title}）` : parent.label;
        }
      }
    }
    if (cur.isSplit) {
      stack.push(cur);
    }
  }

  // Apply forward links on `kepan-split` (process items right-to-left so earlier
  // segment splits don't shift nodes needed by later items in the same paragraph).
  for (const n of nodes) {
    if (!n.isSplit || !n.items.length) continue;
    const matched = n.items.filter((it) => it.targetLb);
    if (!matched.length) continue;
    // Re-collect segments fresh for each wrap from right to left so multiple items
    // in the same text node are wrapped accurately.
    for (let i = matched.length - 1; i >= 0; i -= 1) {
      const it = matched[i];
      const { segs } = collectTextSegments(n.el);
      wrapRange(segs, it.start, it.end, it.targetLb, `跳转到：${it.targetTitle}`);
    }
  }

  // Apply back-to-parent buttons on matched children (text rendered via CSS ::before
  // from data-label so copying paragraph text never copies the button text).
  for (const n of nodes) {
    if (!n.parentLb) continue;
    appendBackButton(n.el, {
      lb: n.parentLb,
      label: `↑ ${n.parentLabel || "上级"}`,
      tooltip: `返回父科判：${n.parentTitle || n.parentLabel}`,
    });
  }

  return { nodes };
}

function appendBackButton(el, { lb, path = null, label, tooltip }) {
  if (el.querySelector("button.kepan-back")) return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = path ? "kepan-back kepan-cross" : "kepan-back";
  btn.dataset.kepanLb = lb;
  if (path) btn.dataset.kepanPath = path;
  btn.dataset.label = label;
  btn.title = tooltip;
  btn.setAttribute("aria-label", tooltip);
  const host = el.querySelector(":scope > .kepan-split-head") || el;
  host.appendChild(btn);
}

/**
 * Asynchronously resolve unmatched `kepan-split` sub-items and parentless child
 * kepans against sibling documents in the same series or folder.
 */
export async function annotateCrossDocKepanLinks(docEl, localResult, { lib, path, volumes, signal } = {}) {
  if (!docEl || !localResult?.nodes?.length || !lib || !path || !volumes || volumes.length < 2) return;

  const needsCross = localResult.nodes.some(
    (n) => (n.isSplit && n.items.some((it) => !it.targetLb)) || (!n.parentLb && n.level > 0),
  );
  if (!needsCross) return;

  const cacheKey = `${lib}|${volumes.map((v) => v.path).join("\n")}`;
  let promise = siblingCacheGet(cacheKey);
  if (!promise) {
    promise = api.siblingKepans(lib, path).catch((err) => {
      siblingCache.delete(cacheKey);
      throw err;
    });
    siblingCacheSet(cacheKey, promise);
  }

  let siblings;
  try {
    siblings = await promise;
  } catch {
    return;
  }
  if (signal?.aborted || !docEl.isConnected || !Array.isArray(siblings) || siblings.length < 2) return;

  // Build one continuous kepan sequence across all selected sibling documents.
  const allNodes = [];
  const curByLb = new Map();
  for (const doc of siblings) {
    for (const k of doc.kepans || []) {
      const parsed = parseKepanText(k.text || "", Boolean(k.is_split));
      const node = {
        path: doc.path,
        docTitle: doc.title,
        lb: k.lb,
        isSplit: Boolean(k.is_split),
        ...parsed,
        parent: null,
      };
      allNodes.push(node);
      if (doc.path === path && k.lb) {
        curByLb.set(k.lb, node);
      }
    }
  }
  if (!curByLb.size) return;

  // Run the level-scoped stack across the concatenated sibling sequence.
  // Repeated identical `kepan-split` recaps at the start of later lessons share
  // the same logical frame so children in later lessons link back to earlier splits too.
  const stack = [];
  for (const cur of allNodes) {
    while (stack.length) {
      const top = stack[stack.length - 1];
      if (top.level > cur.level) {
        stack.pop();
      } else if (top.level === cur.level) {
        const isRecap =
          cur.isSplit &&
          top.num === cur.num &&
          (titlesCompatible(top.title, cur.title) || (!normalizeTitle(top.title) && !normalizeTitle(cur.title)));
        if (isRecap) break;
        stack.pop();
      } else {
        break;
      }
    }

    if (cur.isSplit && stack.length && stack[stack.length - 1].level === cur.level && stack[stack.length - 1].num === cur.num) {
      const frame = stack[stack.length - 1];
      const primary = frame.owners[0];
      if (!cur.parent && primary.parent) {
        cur.parent = { ...primary.parent };
      }
      for (let idx = 0; idx < cur.items.length && idx < primary.items.length; idx += 1) {
        const src = primary.items[idx];
        const dst = cur.items[idx];
        if (!dst.targetLb && src.targetLb) {
          dst.targetPath = src.targetPath;
          dst.targetDocTitle = src.targetDocTitle;
          dst.targetLb = src.targetLb;
          dst.targetTitle = src.targetTitle;
        }
      }
      frame.owners.push(cur);
      continue;
    }

    const frame = stack.length ? stack[stack.length - 1] : null;
    const parent = frame ? frame.owners[frame.owners.length - 1] : null;
    if (parent && parent.isSplit && parent.items.length && cur.level > 0) {
      const expectedLevel = frame.level + 1;
      if (cur.level === expectedLevel) {
        const sameDoc = parent.path === cur.path;
        let itemIdx = parent.items.findIndex(
          (it) => !it.targetLb && it.childNum === cur.num && titlesCompatible(it.childTitle, cur.title),
        );
        if (itemIdx < 0 && sameDoc) {
          itemIdx = parent.items.findIndex((it) => !it.targetLb && it.childNum === cur.num);
        }
        if (itemIdx < 0) {
          itemIdx = parent.items.findIndex((it) => !it.targetLb && titlesCompatible(it.childTitle, cur.title));
        }
        if (itemIdx < 0 && !sameDoc) {
          itemIdx = parent.items.findIndex((it) => !it.targetLb && !normalizeTitle(it.childTitle) && it.childNum === cur.num);
        }
        if (itemIdx >= 0 && cur.lb && parent.lb) {
          const targetTitle = cur.label + (cur.title ? `、${cur.title}` : "");
          for (const owner of frame.owners) {
            const it = owner.items[itemIdx];
            if (it && !it.targetLb) {
              it.targetPath = cur.path;
              it.targetDocTitle = cur.docTitle;
              it.targetLb = cur.lb;
              it.targetTitle = targetTitle;
            }
          }
          cur.parent = {
            path: parent.path,
            docTitle: parent.docTitle,
            lb: parent.lb,
            label: parent.label,
            title: parent.title ? `${parent.label}（${parent.title}）` : parent.label,
          };
        }
      }
    }
    if (cur.isSplit) {
      stack.push({ level: cur.level, num: cur.num, title: cur.title, owners: [cur] });
    }
  }

  // Patch unmatched forward links & back buttons in the current document's DOM.
  for (const localNode of localResult.nodes) {
    const crossNode = curByLb.get(localNode.lb);
    if (!crossNode) continue;

    if (localNode.isSplit && localNode.items.length && crossNode.items.length) {
      const toWrap = [];
      for (let i = 0; i < localNode.items.length && i < crossNode.items.length; i += 1) {
        const localIt = localNode.items[i];
        const crossIt = crossNode.items[i];
        if (!localIt.targetLb && crossIt.targetLb && crossIt.targetPath && crossIt.targetPath !== path) {
          toWrap.push({ localIt, crossIt });
        }
      }
      for (let i = toWrap.length - 1; i >= 0; i -= 1) {
        const { localIt, crossIt } = toWrap[i];
        const { segs } = collectTextSegments(localNode.el);
        wrapRange(
          segs,
          localIt.start,
          localIt.end,
          crossIt.targetLb,
          `跳转到《${crossIt.targetDocTitle}》：${crossIt.targetTitle}`,
          crossIt.targetPath,
        );
      }
    }

    if (!localNode.parentLb && crossNode.parent && crossNode.parent.path !== path) {
      const p = crossNode.parent;
      const shortDoc = p.docTitle.length > 8 ? `${p.docTitle.slice(0, 8)}…` : p.docTitle;
      appendBackButton(localNode.el, {
        lb: p.lb,
        path: p.path,
        label: `↑ ${shortDoc}·${p.label || "上级"}`,
        tooltip: `返回《${p.docTitle}》父科判：${p.title || p.label}`,
      });
    }
  }
}
