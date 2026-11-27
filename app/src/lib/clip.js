// A clipping as Markdown: the selected text as a quote, and a last line with
// its source as a link that leads back to the place (`smartlib://open?…`,
// opened by the reader).
import * as api from "./api.js";

const cut = (s, n) => (s.length > n ? s.slice(0, n) : s);

/** The address of a place in a document. */
export function sourceUrl({ lib, book, path, lb, quote }) {
  const q = new URLSearchParams({ lib, path });
  if (book) q.set("book", book);
  if (lb) q.set("lb", lb);
  if (quote) q.set("q", cut(quote.replace(/\s+/g, ""), 40));
  return "smartlib://open?" + q.toString();
}

/** `{ lib, bookId, path, lb, query }` of an address made by `sourceUrl`, or null. */
export function parseSource(url) {
  try {
    const u = new URL(url);
    const p = u.searchParams;
    if (u.protocol !== "smartlib:" || !p.get("lib")) return null;
    return {
      lib: p.get("lib"),
      bookId: p.get("book") || null,
      path: p.get("path") || null,
      lb: p.get("lb") || null,
      query: p.get("q") || null,
      phrase: !!p.get("q"),
    };
  } catch {
    return null;
  }
}

/** Where a clipping comes from: the label of its source and the place in it. */
async function describe(vol, lb) {
  let where = lb || "";
  try {
    // CBETA: the citation (T15, no. 614, p. 269c29); others: the library and the line
    const c = await api.citeText(vol.lib, vol.path, vol.book_id, lb, vol.title);
    const m = c.match(/^\((.*)\)$/);
    if (m) where = m[1];
  } catch {}
  const title = vol.label ? `${vol.title} ${vol.label}` : vol.title;
  return { title, where, label: `《${title}》${vol.author ? " " + vol.author : ""}${where ? " · " + where : ""}` };
}

/** The excerpt and source of one highlighted row in the search results. */
export function searchClip(hit, snippet) {
  const vol = {
    lib: hit.lib, path: hit.path, book_id: hit.book_id, title: hit.title,
    label: hit.book_id ? hit.subtitle : "", libraryTitle: hit.library_title,
  };
  const first = snippet.parts.findIndex((part) => part.hl);
  return {
    vol, lb: snippet.lb, position: snippet.pos,
    anchor: first < 0 ? "" : snippet.parts.slice(first).map((part) => part.t).join("").trim(),
    text: snippet.parts.map((part) => part.t).join("").trim(),
  };
}

/** A clipping as a node of a mind map: its text, the link to the source, a tag. */
export async function clipNode({ text, vol, lb, anchor = text }) {
  const flat = text.replace(/\s+/g, " ").trim();
  if (vol?.url) {
    return {
      topic: flat.length > 160 ? flat.slice(0, 160) + "…" : flat,
      link: vol.url,
      tag: vol.title || vol.url,
    };
  }
  const d = await describe(vol, lb);
  return {
    topic: flat.length > 160 ? flat.slice(0, 160) + "…" : flat,
    link: sourceUrl({ lib: vol.lib, book: vol.book_id, path: vol.path, lb, quote: anchor }),
    tag: d.label,
  };
}

/** The Markdown to add to a note. `vol` is the document read, `lb` the line the text starts at. */
export async function clipMarkdown({ text, vol, lb, comment = "", anchor = text, position = null }) {
  const quote = text
    .replace(/\r/g, "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => "> " + s)
    .join("\n");
  if (vol?.url) {
    const label = (vol.title || vol.url).replace(/[\[\]]/g, (c) => (c === "[" ? "（" : "）"));
    return `${quote}\n>\n> —— [${label}](${vol.url})` + (comment.trim() ? `\n\n${comment.trim()}` : "");
  }
  let where = lb || "";
  try {
    // CBETA: the citation (T15, no. 614, p. 269c29); others: the library and the line
    const c = await api.citeText(vol.lib, vol.path, vol.book_id, lb, vol.title);
    const m = c.match(/^\((.*)\)$/);
    if (m) where = m[1];
  } catch {}
  const title = vol.label ? `${vol.title} ${vol.label}` : vol.title;
  const source = [vol.libraryTitle, where, position == null ? "" : `第${position}字`].filter(Boolean).join(" · ");
  const label = `《${title}》${vol.author ? " " + vol.author : ""}${source ? " · " + source : ""}`.replace(/[\[\]]/g, (c) => (c === "[" ? "（" : "）"));
  const url = sourceUrl({ lib: vol.lib, book: vol.book_id, path: vol.path, lb, quote: anchor });
  return `${quote}\n>\n> —— [${label}](${url})` + (comment.trim() ? `\n\n${comment.trim()}` : "");
}
