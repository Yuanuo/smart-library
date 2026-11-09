// Showing text in simplified characters. The table (traditional -> simplified, one character to one
// character) comes from the program once; text keeps its length, so the positions of hits, anchors and
// selections stay valid. Only the shown text changes: the files and the index are not touched.
import * as api from "./api.js";

const state = $state({ map: null });
let loading = false;

/** Start reading the table (at once when called again). */
export function ensureSimplifier() {
  if (state.map || loading) return;
  loading = true;
  api
    .t2sPairs()
    .then((s) => {
      const chars = [...s];
      const m = new Map();
      for (let i = 0; i + 1 < chars.length; i += 2) m.set(chars[i], chars[i + 1]);
      state.map = m;
    })
    .catch(() => {})
    .finally(() => (loading = false));
}

/** Plain text in simplified characters (unchanged while the table is not read yet). */
export function simplifyText(text) {
  const m = state.map;
  if (!m || !text) return text;
  let out = "";
  for (const c of text) out += m.get(c) ?? c;
  return out;
}

/** HTML with its text and its `title` attributes in simplified characters; tags, paths and ids stay. */
export function simplifyHtml(html) {
  const m = state.map;
  if (!m || !html) return html;
  return html
    .replace(/>([^<]+)/g, (_, t) => ">" + simplifyText(t))
    .replace(/ (title|data-tip)="([^"]*)"/g, (_, k, v) => ` ${k}="${simplifyText(v)}"`);
}
