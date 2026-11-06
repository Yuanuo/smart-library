// The reading workspace: dockview tabs that can be split, docked, floated
// inside the window, or popped out into their own system window.
//
// Rules (design-spec 9a):
//  * one tab per volume: the tab id is the volume path; opening a volume that
//    is already open (in a tab or a pop-out window) focuses it and jumps;
//  * reading progress (top line `lb` and view mode) is kept per volume, also
//    after the tab is closed, and restored when the volume is opened again;
//  * the layout, the open tabs and the pop-out windows are saved to
//    `workspace.json` in the app data directory and restored at start-up.
// The main window owns that file; pop-out windows report to it with events.

import { createDockview } from "dockview-core";
import { mount, unmount } from "svelte";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as api from "./api.js";
import ReaderPanel from "./ReaderPanel.svelte";
import Results from "./Results.svelte";

const WORKSPACE_VERSION = 1;
const MAX_PROGRESS = 500;

let dv = null; // DockviewApi
let hostEl = null; // the element holding the dockview
let hooks = {};
let progress = {}; // path -> { lb, mode, t }
let popouts = {}; // path -> { label, bookId, title }
let restoring = false;
let saveTimer = null;
let defaultMode = "para";

export const ws = $state({ empty: true });

function labelFor(path) {
  return "reader-" + path.replace(/[^A-Za-z0-9]/g, "_");
}

// ------------------------------------------------------------ persistence

function scheduleSave() {
  if (restoring) return;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, 800);
}

async function save() {
  if (!dv) return;
  const entries = Object.entries(progress).sort((a, b) => (b[1].t || 0) - (a[1].t || 0));
  progress = Object.fromEntries(entries.slice(0, MAX_PROGRESS));
  const layout = dv.toJSON();
  try {
    await api.saveWorkspace({ version: WORKSPACE_VERSION, layout, progress, popouts, defaultMode });
  } catch (e) {
    console.warn("save workspace", e);
  }
}

function recordProgress(path, lb, mode) {
  progress[path] = { lb, mode, t: Date.now() };
  if (mode) defaultMode = mode;
  scheduleSave();
}

// ------------------------------------------------------------ panels

function readerRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  let p = null;
  let panelApi = null;
  return {
    element,
    init(params) {
      panelApi = params.api;
      const prm = params.params || {};
      const saved = progress[prm.path];
      // after a restart (or reopening a closed volume) go back to where the
      // reader was, quietly; an explicit target (search hit) wins
      const state = $state({
        bookId: prm.bookId,
        path: prm.path,
        query: prm.query || null,
        phrase: !!prm.phrase,
        target: prm.target ?? (prm.query ? null : saved?.lb ?? null),
        quiet: prm.target == null && !prm.query,
        nonce: 0,
        mode: prm.mode || saved?.mode || defaultMode,
      });
      p = state;
      instance = mount(ReaderPanel, {
        target: element,
        props: {
          p,
          onopen: (req) => openVolume(req),
          onprogress: (lb, mode) => recordProgress(p.path, lb, mode),
          ontitle: (t) => panelApi?.setTitle(t),
        },
      });
    },
    update(event) {
      const q = event.params || {};
      if (!p) return;
      if ("query" in q) p.query = q.query || null;
      if ("phrase" in q) p.phrase = !!q.phrase;
      if ("target" in q) {
        p.target = q.target;
        p.quiet = false;
        p.nonce += 1;
      }
    },
    dispose() {
      if (instance) unmount(instance);
    },
  };
}

function resultsRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  return {
    element,
    init() {
      instance = mount(Results, { target: element, props: hooks.resultsProps });
    },
    dispose() {
      if (instance) unmount(instance);
    },
  };
}

function watermark() {
  const element = document.createElement("div");
  element.className = "dock-watermark";
  element.innerHTML =
    "<p>从左侧目录打开一部经，或在上方搜索。</p>" +
    "<p class='muted'>标签可拖到边缘分栏停靠；右键标签可浮动或在新窗口打开。</p>";
  return { element, init() {} };
}

function tabMenu({ panel, group, api: dapi }) {
  const isReader = panel.id !== "search";
  const where = group.api.location.type;
  const items = [
    { label: "关闭", action: () => panel.api.close() },
    {
      label: "关闭其他标签",
      action: () => group.panels.filter((x) => x !== panel).forEach((x) => x.api.close()),
    },
    "separator",
  ];
  if (where === "grid" && group.panels.length > 1) {
    items.push({ label: "向右分栏", action: () => split(panel, "right") });
    items.push({ label: "向下分栏", action: () => split(panel, "below") });
  }
  if (where === "floating") {
    items.push({ label: "停靠回主区", action: () => dockBack(panel) });
  } else {
    items.push({ label: "浮动", action: () => float(panel) });
  }
  if (isReader) {
    items.push({ label: "在新窗口打开", action: () => popOut(panel) });
  }
  return items;
}

/** Float a tab over the workspace, at a size large enough to read. */
function float(panel) {
  const box = hostEl?.getBoundingClientRect() || { width: 1000, height: 700 };
  const width = Math.max(420, Math.round(box.width * 0.45));
  const height = Math.max(360, Math.round(box.height * 0.7));
  dv.addFloatingGroup(panel, { x: Math.round(box.width - width - 40), y: 40, width, height });
}

/** Move a tab into a new group beside (or below) its group, for comparing. */
function split(panel, direction) {
  const g = dv.addGroup({ referenceGroup: panel.group, direction });
  panel.api.moveTo({ group: g, position: "center" });
}

function dockBack(panel) {
  const target = dv.groups.find((g) => g.api.location.type === "grid" && g !== panel.group);
  if (target) panel.api.moveTo({ group: target, position: "center" });
  else panel.api.moveTo({ position: "right" });
}

// ------------------------------------------------------------ pop-out windows

function popoutUrl(info) {
  return "/?popout=" + encodeURIComponent(JSON.stringify(info));
}

async function createPopout(path, bookId, title) {
  const label = labelFor(path);
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return;
  }
  const saved = progress[path] || {};
  new WebviewWindow(label, {
    url: popoutUrl({ bookId, path, lb: saved.lb || null, mode: saved.mode || defaultMode, title }),
    title: `${title} — Smart Library`,
    width: 960,
    height: 860,
  });
  popouts[path] = { label, bookId, title };
  scheduleSave();
}

async function popOut(panel) {
  const prm = panel.params || {};
  const path = prm.path;
  const title = panel.title || path;
  panel.api.close();
  await createPopout(path, prm.bookId, title);
}

function listenToPopouts() {
  listen("popout-progress", (e) => recordProgress(e.payload.path, e.payload.lb, e.payload.mode));
  listen("popout-closed", (e) => {
    delete popouts[e.payload.path];
    scheduleSave();
  });
  listen("popout-dock", (e) => {
    delete popouts[e.payload.path];
    openVolume({ bookId: e.payload.bookId, path: e.payload.path, lb: e.payload.lb });
  });
  listen("popout-open", (e) => openVolume(e.payload));
}

// ------------------------------------------------------------ public

/** Create the dockview in `element` and restore the saved workspace. */
export async function init(element, h) {
  hooks = h;
  hostEl = element;
  dv = createDockview(element, {
    className: "dockview-theme-light sl-dock",
    createComponent: (opts) => (opts.name === "results" ? resultsRenderer() : readerRenderer()),
    createWatermarkComponent: watermark,
    getTabContextMenuItems: tabMenu,
  });
  dv.onDidLayoutChange(scheduleSave);
  const refreshEmpty = () => (ws.empty = dv.panels.length === 0);
  dv.onDidAddPanel(refreshEmpty);
  dv.onDidRemovePanel(refreshEmpty);
  listenToPopouts();

  const saved = await api.loadWorkspace().catch(() => null);
  if (saved && saved.version === WORKSPACE_VERSION) {
    progress = saved.progress || {};
    defaultMode = saved.defaultMode || "para";
    restoring = true;
    try {
      if (saved.layout) dv.fromJSON(saved.layout);
    } catch (e) {
      console.warn("restore layout", e);
      dv.clear();
    }
    // search results are not saved
    dv.getPanel("search")?.api.close();
    restoring = false;
    for (const [path, w] of Object.entries(saved.popouts || {})) {
      await createPopout(path, w.bookId, w.title);
    }
  }
  refreshEmpty();
}

/**
 * Open a volume: focus its tab (or pop-out window) if it is open, else open a
 * new tab in the active group. `lb` jumps to a line; `query`/`phrase`
 * highlight the hits.
 */
export async function openVolume({ bookId, path = null, lb = null, query = null, phrase = false }) {
  const id = await api.resolveVolume(bookId, path);
  if (popouts[id.path]) {
    const w = await WebviewWindow.getByLabel(popouts[id.path].label);
    if (w) {
      await w.unminimize().catch(() => {});
      await w.setFocus();
      await emitTo(popouts[id.path].label, "popout-jump", { lb, query, phrase });
      return;
    }
    delete popouts[id.path];
  }
  const panel = dv.getPanel(id.path);
  if (panel) {
    panel.api.setActive();
    panel.api.updateParameters({ target: lb, query, phrase });
    return;
  }
  dv.addPanel({
    id: id.path,
    component: "reader",
    title: id.title,
    params: { bookId: id.book_id, path: id.path, target: lb, query, phrase },
  });
}

/** Show (or focus) the search results tab. */
export function showResults() {
  const panel = dv.getPanel("search");
  if (panel) {
    panel.api.setActive();
    return;
  }
  dv.addPanel({ id: "search", component: "results", title: "搜索结果" });
}

/** Save now (e.g. before the window closes). */
export function flush() {
  clearTimeout(saveTimer);
  return save();
}
