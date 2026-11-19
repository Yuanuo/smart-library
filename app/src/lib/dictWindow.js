import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { emitTo } from "@tauri-apps/api/event";

let dictSeq = 0;
const SELECT_LABEL = "dict-select";

/**
 * Open the dictionary window. `reuse` sends the term to one shared window
 * (select-to-look-up): selecting word after word updates it in place instead
 * of piling up a window for every word. Its `dict-query` listener does the
 * search.
 */
export async function openDictWindow(query = "", { reuse = false } = {}) {
  if (reuse) {
    // A window that is already closing cannot take the query; then a new one
    // is opened below.
    try {
      const existing = await WebviewWindow.getByLabel(SELECT_LABEL);
      if (existing) {
        await emitTo(SELECT_LABEL, "dict-query", { query });
        const title = query ? `查词典：${query} — Smart Library` : "查词典 — Smart Library";
        await existing.setTitle(title).catch(() => {});
        await existing.unminimize().catch(() => {});
        await existing.setFocus().catch(() => {});
        return existing;
      }
    } catch {}
  }
  const label = reuse ? SELECT_LABEL : `dict-win-${Date.now()}-${++dictSeq}`;
  new WebviewWindow(label, {
    url: "/?dict=" + encodeURIComponent(JSON.stringify({ query })),
    title: query ? `查词典：${query} — Smart Library` : "查词典 — Smart Library",
    width: 1060,
    height: 740,
  });
}

export async function openDictEntryWindow({ dictId, entryId, title }) {
  const safeId = String(dictId).replace(/[^a-zA-Z0-9_-]/g, "_");
  const label = `dict-entry-${safeId}-${entryId}`;
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.unminimize().catch(() => {});
    await existing.setFocus();
    return;
  }
  new WebviewWindow(label, {
    url: "/?dictentry=" + encodeURIComponent(JSON.stringify({ dictId, entryId, title })),
    title: `${title || "词条详情"} — Smart Library`,
    width: 760,
    height: 640,
  });
}
