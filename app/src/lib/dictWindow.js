import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

let dictSeq = 0;

export async function openDictWindow(query = "") {
  const label = `dict-win-${Date.now()}-${++dictSeq}`;
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
