// Panels that can live in a window of their own (recent, collections and
// tags). Such a window shows the same component as the dialog; what it opens
// is reported to the main window with events (PanelWindow.svelte).
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

const PANELS = {
  recent: { title: "最近访问", width: 820, height: 680 },
  marks: { title: "收藏与标签", width: 1000, height: 720 },
};

export async function openPanelWindow(kind) {
  const label = `panel-${kind}`;
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.unminimize().catch(() => {});
    await existing.setFocus();
    return;
  }
  const p = PANELS[kind];
  new WebviewWindow(label, {
    url: `/?panel=${kind}`,
    title: `${p.title} — Smart Library`,
    width: p.width,
    height: p.height,
  });
}
