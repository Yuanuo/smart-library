// The note that clippings go to by default: the last one a clipping was
// added to or that was edited. Kept in local storage.
const KEY = "sl.clipTarget";

/** `{ lib, id, title }` or null. */
export function getLast() {
  try {
    const t = JSON.parse(localStorage.getItem(KEY) || "null");
    return t && t.lib && t.id ? t : null;
  } catch {
    return null;
  }
}

export function setLast(t) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ lib: t.lib, id: String(t.id), title: t.title }));
  } catch {}
}
