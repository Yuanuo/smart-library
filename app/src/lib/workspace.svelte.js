// The reading workspace: dockview tabs that can be split, docked, floated
// inside the window, or popped out into their own system window.
//
// Rules (design-spec 9a):
//  * one tab per document: the tab id is `<library>|<path>` (a CBETA volume
//    or an HTML article); opening a document that is already open (in a tab
//    or a pop-out window) focuses it and jumps;
//  * reading progress (top line `lb` and view mode) is kept per document,
//    also after the tab is closed, and restored when it is opened again;
//  * the layout, the open tabs and the pop-out windows are saved to
//    `workspace.json` in the app data directory and restored at start-up.
// The main window owns that file; pop-out windows report to it with events.

import { createDockview } from "dockview-core";
import { mount, unmount } from "svelte";
import { emitTo, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as api from "./api.js";
import ReaderPanel from "./ReaderPanel.svelte";
import Results from "./Results.svelte";

// 2: tabs are `<library>|<path>` (version 1 had CBETA paths only)
const WORKSPACE_VERSION = 2;
const MAX_PROGRESS = 500;

let dv = null; // DockviewApi
let hostEl = null; // the element holding the dockview
let hooks = {};
let progress = {}; // tab -> { lb, mode, t }
let popouts = {}; // tab -> { label, lib, bookId, path, title }
let tooltips = {}; // tab -> tooltip of its tab (title, author, path)
let restoring = false;
let saveTimer = null;
let defaultMode = "para";

export const ws = $state({ empty: true, current: null, opened: 0 }); // current: { title, lib } of the active reading tab

function labelFor(tab) {
  return "reader-" + tab.replace(/[^A-Za-z0-9]/g, "_");
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

const historyTimers = {};

function recordProgress(tab, lb, mode) {
  progress[tab] = { lb, mode, t: Date.now() };
  if (mode) defaultMode = mode;
  scheduleSave();
  // the history keeps the position of every document ever opened
  clearTimeout(historyTimers[tab]);
  historyTimers[tab] = setTimeout(() => {
    const i = tab.indexOf("|");
    api.recentProgress(tab.slice(0, i), tab.slice(i + 1), lb, mode || null).catch(() => {});
  }, 1500);
}

/** Remember where a document was left (from the history), so that opening it
 * goes back there quietly. */
export function seedProgress(lib, path, lb, mode) {
  const tab = `${lib}|${path}`;
  if (lb) progress[tab] = { lb, mode: mode || defaultMode, t: Date.now() };
}

// ------------------------------------------------------------ panels

function readerRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  let p = null;
  let panelApi = null;
  // dockview takes a hidden tab's element out of the page, which resets its
  // scroll position: remember it while visible, put it back when shown
  let scrollTop = 0;
  // a tab restored after a restart but never shown has no scroll position
  // yet (its content was laid out while hidden): go to the saved line when
  // it is first shown
  let positioned = false;
  element.addEventListener(
    "scroll",
    (e) => {
      if (element.isConnected && e.target.classList?.contains("scroll")) {
        scrollTop = e.target.scrollTop;
        if (scrollTop > 0) positioned = true;
      }
    },
    true,
  );
  return {
    element,
    init(params) {
      panelApi = params.api;
      panelApi.onDidVisibilityChange(({ isVisible }) => {
        if (!isVisible) return;
        if (!positioned && p?.target) {
          positioned = true;
          p.nonce += 1;
          return;
        }
        const top = scrollTop;
        requestAnimationFrame(() => {
          const s = element.querySelector(".scroll");
          if (s && top && Math.abs(s.scrollTop - top) > 1) s.scrollTop = top;
        });
      });
      const prm = params.params || {};
      const tab = `${prm.lib}|${prm.path}`;
      const saved = progress[tab];
      // after a restart (or reopening a closed document) go back to where
      // the reader was, quietly; an explicit target (search hit) wins
      const state = $state({
        lib: prm.lib,
        bookId: prm.bookId ?? null,
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
          onlocate: () => hooks.onlocate?.({ lib: state.lib, bookId: state.bookId, path: state.path }),
          onaction: (a) => hooks.onaction?.(a),
          onprogress: (lb, mode) => recordProgress(tab, lb, mode),
          ontitle: (t, tooltip) => {
            panelApi?.setTitle(t);
            if (panelApi?.isActive) ws.current = { title: t, lib: state.lib, path: state.path, bookId: state.bookId };
            tooltips[panelApi.id] = tooltip;
            applyTooltip(panelApi.id);
          },
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
      if (panelApi) delete tooltips[panelApi.id];
      if (instance) unmount(instance);
    },
  };
}

/** The tooltip of a reading tab (the tab element may be made again when
 * the tab moves, so it is set again after layout changes). */
function applyTooltip(id) {
  const el = dv?.getPanel(id)?.view?.tab?.element;
  if (el && tooltips[id] && el.title !== tooltips[id]) el.title = tooltips[id];
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
    "<p>从左侧目录打开一部经或一篇文章，或在上方搜索。</p>" +
    "<p class='muted'>标签可拖到边缘分栏停靠；右键标签可分栏、浮动或在新窗口打开。</p>";
  return { element, init() {} };
}

function tabMenu({ panel, group }) {
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
    items.push("separator");
    items.push({
      label: "在目录树中定位",
      action: () => {
        const prm = panel.params || {};
        hooks.onlocate?.({ lib: prm.lib, bookId: prm.bookId, path: prm.path });
      },
    });
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

async function createPopout({ tab, lib, bookId, path, title }) {
  const label = labelFor(tab);
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return;
  }
  const saved = progress[tab] || {};
  new WebviewWindow(label, {
    url: popoutUrl({ tab, lib, bookId, path, lb: saved.lb || null, mode: saved.mode || defaultMode, title }),
    title: `${title} — Smart Library`,
    width: 960,
    height: 860,
  });
  popouts[tab] = { label, lib, bookId, path, title };
  scheduleSave();
}

async function popOut(panel) {
  const prm = panel.params || {};
  const title = panel.title || prm.path;
  panel.api.close();
  await createPopout({ tab: panel.id, lib: prm.lib, bookId: prm.bookId, path: prm.path, title });
}

function listenToPopouts() {
  listen("popout-progress", (e) => recordProgress(e.payload.tab, e.payload.lb, e.payload.mode));
  listen("popout-closed", (e) => {
    delete popouts[e.payload.tab];
    scheduleSave();
  });
  listen("popout-dock", (e) => {
    const q = e.payload;
    delete popouts[q.tab];
    openVolume({ lib: q.lib, bookId: q.bookId, path: q.path, lb: q.lb });
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
  dv.onDidLayoutChange(() => {
    scheduleSave();
    for (const id of Object.keys(tooltips)) applyTooltip(id);
  });
  const refreshEmpty = () => (ws.empty = dv.panels.length === 0);
  dv.onDidActivePanelChange((panel) => {
    ws.current =
      panel && panel.id !== "search" && panel.params?.lib
        ? { title: panel.title, lib: panel.params.lib, path: panel.params.path, bookId: panel.params.bookId ?? null }
        : null;
  });
  dv.onDidAddPanel(refreshEmpty);
  dv.onDidRemovePanel(refreshEmpty);
  listenToPopouts();

  const saved = await api.loadWorkspace().catch(() => null);
  if (saved && saved.version === 1) {
    // CBETA paths of version 1 belong to the library `cbeta` (migrated
    // registry); the layout is rebuilt by the user
    progress = Object.fromEntries(Object.entries(saved.progress || {}).map(([k, v]) => [`cbeta|${k}`, v]));
    defaultMode = saved.defaultMode || "para";
  } else if (saved && saved.version === WORKSPACE_VERSION) {
    progress = saved.progress || {};
    defaultMode = saved.defaultMode || "para";
    const known = new Set((h.libraries || []).map((l) => l.id));
    restoring = true;
    try {
      if (saved.layout) dv.fromJSON(saved.layout);
    } catch (e) {
      console.warn("restore layout", e);
      dv.clear();
    }
    // search results are not saved; tabs of unbound libraries are dropped
    dv.getPanel("search")?.api.close();
    for (const panel of [...dv.panels]) {
      const lib = panel.params?.lib;
      if (lib && !known.has(lib)) panel.api.close();
    }
    restoring = false;
    for (const [tab, w] of Object.entries(saved.popouts || {})) {
      if (known.has(w.lib)) await createPopout({ tab, ...w });
    }
  }
  refreshEmpty();
}

/**
 * Open a document: focus its tab (or pop-out window) if it is open, else
 * open a new tab in the active group. CBETA: `bookId` (+ `path` of a
 * volume); HTML libraries: `path` of the article. `lb` jumps to a line or
 * paragraph; `query`/`phrase` highlight the hits.
 */
export async function openVolume({ lib, bookId = null, path = null, lb = null, query = null, phrase = false }) {
  const id = await api.resolveNode(lib, bookId, path);
  api.recentTouch(id.lib, id.path, id.book_id, id.title).then(() => ws.opened++).catch(() => {});
  if (popouts[id.tab]) {
    const w = await WebviewWindow.getByLabel(popouts[id.tab].label);
    if (w) {
      await w.unminimize().catch(() => {});
      await w.setFocus();
      await emitTo(popouts[id.tab].label, "popout-jump", { lb, query, phrase });
      return;
    }
    delete popouts[id.tab];
  }
  const panel = dv.getPanel(id.tab);
  if (panel) {
    panel.api.setActive();
    panel.api.updateParameters({ target: lb, query, phrase });
    return;
  }
  dv.addPanel({
    id: id.tab,
    component: "reader",
    title: id.title,
    params: { lib: id.lib, bookId: id.book_id, path: id.path, target: lb, query, phrase },
  });
}

/** Close the tab of one document (a deleted note). */
export function closeDocument(lib, path) {
  dv?.getPanel(`${lib}|${path}`)?.api.close();
  scheduleSave();
}

/** Show a new title on the tab of a document (a renamed note). */
export function retitle(lib, path, title) {
  dv?.getPanel(`${lib}|${path}`)?.api.setTitle(title);
}

/** Close every tab and pop-out window of a library (after unbinding it). */
export async function closeLibrary(lib) {
  for (const panel of [...(dv?.panels || [])]) {
    if (panel.params?.lib === lib) panel.api.close();
  }
  for (const [tab, w] of Object.entries(popouts)) {
    if (w.lib === lib) {
      (await WebviewWindow.getByLabel(w.label))?.close();
      delete popouts[tab];
    }
  }
  scheduleSave();
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
