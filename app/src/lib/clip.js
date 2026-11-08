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

/** The Markdown to add to a note. `vol` is the document read, `lb` the line the text starts at. */
export async function clipMarkdown({ text, vol, lb, comment = "" }) {
  const quote = text
    .replace(/\r/g, "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => "> " + s)
    .join("\n");
  let where = lb || "";
  try {
    // CBETA: the citation (T15, no. 614, p. 269c29); others: the library and the line
    const c = await api.citeText(vol.lib, vol.path, vol.book_id, lb, vol.title);
    const m = c.match(/^\((.*)\)$/);
    if (m) where = m[1];
  } catch {}
  const title = vol.label ? `${vol.title} ${vol.label}` : vol.title;
  const label = `《${title}》${vol.author ? " " + vol.author : ""}${where ? " · " + where : ""}`.replace(/[\[\]]/g, (c) => (c === "[" ? "（" : "）"));
  const url = sourceUrl({ lib: vol.lib, book: vol.book_id, path: vol.path, lb, quote: text });
  return `${quote}\n>\n> —— [${label}](${url})` + (comment.trim() ? `\n\n${comment.trim()}` : "");
}
