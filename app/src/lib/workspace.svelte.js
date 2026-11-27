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
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as api from "./api.js";
import ReaderPanel from "./ReaderPanel.svelte";
import FolderPanel from "./FolderPanel.svelte";
import SearchPage from "./SearchPage.svelte";
import AiPanel from "./AiPanel.svelte";
import StartPage from "./StartPage.svelte";
import { formatShortcut, formatCombo } from "./shortcuts.js";
import { getCurrentWindow } from "@tauri-apps/api/window";

// 2: tabs are `<library>|<path>` (version 1 had CBETA paths only)
const WORKSPACE_VERSION = 2;
const MAX_PROGRESS = 500;
const MAX_VIEW_HISTORY = 100;
const FOLDER_BROWSE_ID = "folder:browse";

let dv = null; // DockviewApi
let hostEl = null; // the element holding the dockview
let hooks = {};
let progress = {}; // tab -> { lb, mode, t }
let popouts = {}; // tab -> { label, lib, bookId, path, title }
let tooltips = {}; // tab -> tooltip of its tab (title, author, path)
let states = {}; // tab -> the reactive state of its panel (for notes changed from outside)
let appendSeq = 0;
let searchStates = {}; // tab -> the reactive state of a search page
let searchSeq = 0; // search tabs are `search:<n>`
let lastSearchId = null; // the search tab used last (it receives "add to range")
let searchWinSeq = 0;
let rawWinSeq = 0;
let aiChatStates = {}; // tab -> { sessionId, title }
let aiChatSeq = 0; // ai chat tabs are `aichat:<n>`
let aiChatWinSeq = 0;
let folderStates = {}; // tab -> { lib, rel, key, title, reload }
let folderSeq = 0; // explicit new folder tabs are `folder:<n>`
let infos = {}; // tab -> { title, lib, path, bookId, author } of its document
let docStates = {}; // tab -> { lib, path, bookId, title, contentType, writable, getText, editDoc }
let searchResults = {}; // tab -> { query, hits, totalVolumes, totalBooks }
let restoring = false;
let saveTimer = null;
let defaultMode = "para";
let viewHistory = []; // [{ tab, lib, bookId, path }]
let viewIndex = -1;
let navigatingHistory = false;
let activationOrder = []; // panel ids in MRU order (most recently active last)
let lastActiveId = null;
let pendingCloseTarget = null; // panel id to activate after Dockview's own close fallback

function touchActivation(id) {
  if (!id) return;
  const idx = activationOrder.indexOf(id);
  if (idx >= 0) activationOrder.splice(idx, 1);
  activationOrder.push(id);
}

function dropActivation(id) {
  const idx = activationOrder.indexOf(id);
  if (idx >= 0) activationOrder.splice(idx, 1);
}

function pickNextActiveAfterClose(closedGroupId) {
  let fallback = null;
  for (let i = activationOrder.length - 1; i >= 0; i--) {
    const p = dv?.getPanel(activationOrder[i]);
    if (!p) continue;
    if (closedGroupId && p.group?.id === closedGroupId) return p;
    if (!fallback) fallback = p;
  }
  return fallback;
}

export const ws = $state({
  empty: true,
  current: null,
  activeTab: null, // { kind: "doc" | "search" | "aichat", title?: string } of the active tab
  opened: 0,
  activeDoc: null,
  activeSearch: null,
  canBack: false, // a previous view is in the history
  canForward: false, // a later view is in the history
}); // current: { title, lib } of the active reading tab

export const startState = $state({
  libraryCount: 0,
  aiConfigured: false,
  mcpRunning: false,
});

export function setStartState(patch) {
  Object.assign(startState, patch);
}

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
    await api.saveWorkspace({ version: WORKSPACE_VERSION, layout, progress, popouts, defaultMode, viewHistory, viewIndex });
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
      {
        const prm = params.params || {};
        infos[panelApi.id] = { title: params.title || "", lib: prm.lib, path: prm.path, bookId: prm.bookId ?? null, author: "" };
      }
      panelApi.onDidVisibilityChange(({ isVisible }) => {
        if (p) p.visible = isVisible;
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
        reload: 0, // +1: read the document again
        visible: panelApi.isVisible, // whether this tab is currently shown
        check: 0, // +1: check if a folder file changed outside
        editing: false, // a note is open in its editor
        append: null, // { text }: a clipping for the editor
      });
      p = state;
      states[panelApi.id] = state;
      instance = mount(ReaderPanel, {
        target: element,
        props: {
          p,
          getNodePath: hooks.onnodepath,
          tabMenuItems: getTabMenuItems,
          onopen: (req) => {
            if (req?.kind === "folder" && !req.path && req.folderPath != null) {
              const title = req.title || req.folderPath.split("/").filter(Boolean).pop() || "";
              return openFolder({ lib: req.lib, rel: req.folderPath, title });
            }
            return openVolume(req);
          },
          onlocate: () => hooks.onlocate?.({ lib: state.lib, bookId: state.bookId, path: state.path }),
          onaction: (a) => hooks.onaction?.(a),
          onai: (req) => hooks.onai?.(req),
          ondocstate: (docState) => {
            docStates[panelApi.id] = docState;
            if (panelApi?.isActive) {
              ws.activeDoc = docState;
              ws.activeSearch = null;
            }
          },
          onprogress: (lb, mode) => recordProgress(tab, lb, mode),
          ontitle: (t, tooltip, meta) => {
            panelApi?.setTitle(t);
            infos[panelApi.id] = { title: t, lib: state.lib, path: state.path, bookId: state.bookId, author: meta?.author || "" };
            if (panelApi?.isActive) ws.current = { ...infos[panelApi.id] };
            const id = panelApi.id;
            tooltips[id] = tooltip;
            applyTooltip(id);
            // the path in the tree joins the tooltip when it is known
            Promise.resolve(hooks.ontreepath?.({ lib: state.lib, bookId: state.bookId, path: state.path }))
              .then((tp) => {
                if (!tp || tooltips[id] === undefined) return;
                tooltips[id] = `${tooltip}\n${tp}`;
                applyTooltip(id);
              })
              .catch(() => {});
          },
        },
      });
    },
    update(event) {
      const q = event.params || {};
      if (!p) return;
      if ("query" in q) p.query = q.query || null;
      if ("phrase" in q) p.phrase = !!q.phrase;
      if (q.target != null) {
        p.target = q.target;
        p.quiet = false;
        p.nonce += 1;
      } else if (q.query) {
        p.target = null;
        p.quiet = false;
        p.nonce += 1;
      }
    },
    dispose() {
      if (panelApi) {
        delete states[panelApi.id];
        delete tooltips[panelApi.id];
        delete infos[panelApi.id];
        delete docStates[panelApi.id];
      }
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

const isSearchTab = (id) => typeof id === "string" && id.startsWith("search:");
const isAiChatTab = (id) => typeof id === "string" && id.startsWith("aichat:");
const isFolderTab = (id) => typeof id === "string" && id.startsWith("folder:");

function folderHistoryKey(lib, rel = "", key = "") {
  return `folder:${lib}|${rel || ""}|${key || ""}`;
}

function folderRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  let state = null;
  let panelApi = null;
  return {
    element,
    init(params) {
      panelApi = params.api;
      const prm = params.params || {};
      const next = $state({
        lib: prm.lib || "",
        rel: prm.rel || "",
        key: prm.key || "",
        title: prm.title || params.title || "",
        reload: 0,
      });
      state = next;
      folderStates[panelApi.id] = state;
      instance = mount(FolderPanel, {
        target: element,
        props: {
          p: state,
          getLibrary: (id) => (hooks.libraries || []).find((l) => l.id === id) || null,
          onnavigate: (req, opts = {}) =>
            openFolder(req, { ...opts, panelId: opts?.newTab ? null : panelApi.id }),
          onopen: (req) => openVolume(req),
          onlocate: () =>
            hooks.onlocate?.({ lib: state.lib, rel: state.rel, key: state.key, isFolder: true }),
          ontitle: (t, tooltip) => {
            panelApi?.setTitle(t);
            state.title = t;
            tooltips[panelApi.id] = tooltip;
            applyTooltip(panelApi.id);
            if (panelApi?.isActive) ws.activeTab = { kind: "folder", title: t };
          },
        },
      });
    },
    update(event) {
      const q = event.params || {};
      if (!state) return;
      const nextLib = q.lib ?? state.lib;
      const nextRel = q.rel ?? "";
      const nextKey = q.key ?? "";
      const nextTitle = q.title ?? "";
      const same = nextLib === state.lib && nextRel === state.rel && nextKey === state.key;
      state.lib = nextLib;
      state.rel = nextRel;
      state.key = nextKey;
      if (nextTitle) {
        state.title = nextTitle;
        panelApi?.setTitle(nextTitle);
      }
      if (same || q.forceReload) state.reload += 1;
      scheduleSave();
    },
    dispose() {
      if (panelApi) {
        delete folderStates[panelApi.id];
        delete tooltips[panelApi.id];
      }
      if (instance) unmount(instance);
    },
  };
}

function aiChatRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  let panelApi = null;
  return {
    element,
    init(params) {
      panelApi = params.api;
      const prm = params.params || {};
      const st = {
        sessionId: prm.sessionId || null,
        title: params.title || "AI 助手",
      };
      aiChatStates[panelApi.id] = st;
      instance = mount(AiPanel, {
        target: element,
        props: {
          scopeKind: "chat",
          initialSessionId: st.sessionId,
          onOpenDoc: (req) => openVolume(req).catch((e) => hooks.onaction?.({ type: "error", text: String(e) })),
          onPopout: () => popOutAiChat(panelApi.id),
          onSessionChange: ({ sessionId, title }) => {
            st.sessionId = sessionId;
            if (title) {
              st.title = `AI：${title.slice(0, 14)}`;
              panelApi?.setTitle(st.title);
              if (panelApi?.isActive) ws.activeTab = { kind: "aichat", title: st.title };
            }
            panelApi?.updateParameters({ kind: "aichat", sessionId: st.sessionId });
            scheduleSave();
          },
        },
      });
    },
    update() {},
    dispose() {
      if (panelApi) delete aiChatStates[panelApi.id];
      if (instance) unmount(instance);
    },
  };
}

/** What of a search page is kept with its tab (and handed to a window). */
function searchSnapshot(st) {
  const x = $state.snapshot(st);
  return { kind: "search", query: x.query, phrase: x.phrase, library: x.lib, libraryPinned: x.libraryPinned, filters: x.filters, scopes: x.scopes, tab: x.tab, page: x.page, ran: x.ran };
}

function searchRenderer() {
  const element = document.createElement("div");
  element.className = "panel-host";
  let instance = null;
  let panelApi = null;
  // a hidden tab's element is taken out of the page, which resets scroll
  // positions: remember them while visible, put them back when shown
  const tops = new Map();
  element.addEventListener(
    "scroll",
    (e) => {
      if (element.isConnected && e.target instanceof HTMLElement) tops.set(e.target, e.target.scrollTop);
    },
    true,
  );
  return {
    element,
    init(params) {
      panelApi = params.api;
      const prm = params.params || {};
      const st = $state({
        query: prm.query || "",
        phrase: !!prm.phrase,
        lib: prm.library ?? null, // (not `lib`: a tab with that parameter belongs to a library)
        libraryPinned: !!prm.libraryPinned,
        filters: prm.filters || {},
        scopes: prm.scopes || [],
        tab: prm.tab || "/bulei",
        page: prm.page || 1,
        ran: !!prm.ran,
        autorun: !!prm.autorun, // search once when shown
        rerun: 0, // +1: the range changed from outside, search again
        focus: 0, // +1: put the cursor in the search box
      });
      searchStates[panelApi.id] = st;
      panelApi.onDidVisibilityChange(({ isVisible }) => {
        if (!isVisible) return;
        requestAnimationFrame(() => {
          for (const [el, top] of tops) {
            if (!el.isConnected) tops.delete(el);
            else if (top && Math.abs(el.scrollTop - top) > 1) el.scrollTop = top;
          }
        });
      });
      instance = mount(SearchPage, {
        target: element,
        props: {
          s: st,
          onopen: (req) => openVolume(req).catch((e) => hooks.onaction?.({ type: "error", text: String(e) })),
          onstate: () => {
            panelApi.updateParameters(searchSnapshot(st));
            scheduleSave();
          },
          ontitle: (t) => {
            panelApi.setTitle(t);
            if (panelApi?.isActive) ws.activeTab = { kind: "search", title: t };
          },
          onpopout: () => popOutSearch(panelApi.id),
          onai: (req) => hooks.onai?.(req),
          onsearchresult: (sr) => {
            searchResults[panelApi.id] = sr;
            if (panelApi?.isActive) {
              ws.activeSearch = sr;
              ws.activeDoc = null;
            }
          },
        },
      });
    },
    update() {},
    dispose() {
      if (panelApi) {
        delete searchStates[panelApi.id];
        delete searchResults[panelApi.id];
      }
      if (instance) unmount(instance);
    },
  };
}

/** Workspaces saved before search pages had tabs of their own hold a `results` tab: it is closed at start. */
function oldResultsRenderer() {
  const element = document.createElement("div");
  return { element, init() {}, update() {}, dispose() {} };
}

function watermark() {
  const element = document.createElement("div");
  element.className = "dock-watermark";
  let instance = null;
  return {
    element,
    init() {
      instance = mount(StartPage, {
        target: element,
        props: {
          state: startState,
          actions: hooks.startActions || {},
        },
      });
    },
    dispose() {
      if (instance) {
        unmount(instance);
        instance = null;
      }
    },
  };
}

function tabMenu({ panel, group }) {
  const isSearch = isSearchTab(panel.id);
  const isAiChat = isAiChatTab(panel.id);
  const isFolder = isFolderTab(panel.id);
  const isReader = !isSearch && !isAiChat && !isFolder && panel.id !== "search";
  const where = group.api.location.type;
  const items = [
    { label: formatShortcut("close") ? `关闭（${formatShortcut("close")}）` : "关闭", action: () => panel.api.close() },
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
  if (isSearch) items.push({ label: "在新窗口打开", action: () => popOutSearch(panel.id) });
  if (isAiChat) items.push({ label: "在新窗口打开", action: () => popOutAiChat(panel.id) });
  if (isFolder) {
    items.push("separator", {
      label: "在目录树中定位",
      action: () => {
        const st = folderStates[panel.id] || panel.params || {};
        hooks.onlocate?.({ lib: st.lib, rel: st.rel || "", key: st.key || "", isFolder: true });
      },
    });
  }
  if (isReader) {
    items.push({ label: "在新窗口打开", action: () => popOut(panel) });
    items.push("separator");
    items.push({
      label: "在目录树中定位",
      action: () => {
        const i = infos[panel.id] || panel.params || {};
        hooks.onlocate?.({ lib: i.lib, bookId: i.bookId, path: i.path });
      },
    });
    for (const [label, tree] of [["复制原文件路径", false], ["复制目录树路径", true]]) {
      items.push({
        label,
        action: () => {
          const i = infos[panel.id] || panel.params || {};
          hooks.oncopypath?.({ lib: i.lib, bookId: i.bookId, path: i.path, title: panel.title, tree });
        },
      });
    }
    items.push({
      label: "查看原始（Raw）数据",
      action: () => {
        const i = infos[panel.id] || panel.params || {};
        openRaw({ lib: i.lib, bookId: i.bookId ?? null, path: i.path ?? null, title: panel.title });
      },
    });
  }
  return items;
}

/**
 * Return the tab context menu items for the panel containing `anchorEl`
 * (or the active panel when omitted), reusing the same `tabMenu` builder.
 */
export function getTabMenuItems(anchorEl = null) {
  const panel =
    (anchorEl && dv?.panels?.find((p) => p.view?.content?.element?.contains(anchorEl))) ||
    dv?.activePanel;
  if (!panel?.group) return null;
  return tabMenu({ panel, group: panel.group });
}

/**
 * Show the active tab's context menu at `(x, y)` (client coordinates), the
 * same menu a right-click on the tab itself builds. The reader's title link
 * uses it, so the menu is not defined twice.
 */
export function showTabMenuAt(x, y, anchorEl = null) {
  const panel =
    (anchorEl && dv?.panels?.find((p) => p.view?.content?.element?.contains(anchorEl))) ||
    dv?.activePanel;
  if (!panel) return;
  const tab =
    panel.view?.tab?.element?.closest?.(".dv-tab") ||
    [...(hostEl?.querySelectorAll(".dv-tab") || [])].find((el) => el.dataset?.tabPanelId === panel.id);
  if (!tab) return;
  tab.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: x, clientY: y }));
}

/**
 * Bring the active tab into view. Tabs that do not fit stay in the strip but
 * are clipped (dockview lists them in a dropdown), so activating one of them
 * would otherwise leave its tab out of sight.
 */
function revealActiveTab(panel = dv?.activePanel) {
  const strip = panel?.group?.element?.querySelector(".dv-tabs-container");
  if (!strip) return;
  const tab = [...strip.children].find((el) => el.dataset?.tabPanelId === panel.id);
  if (!tab) return;
  const left = tab.offsetLeft;
  const right = left + tab.offsetWidth;
  if (left < strip.scrollLeft) strip.scrollLeft = left;
  else if (right > strip.scrollLeft + strip.clientWidth) strip.scrollLeft = right - strip.clientWidth;
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

/** A document's original file, read-only, in a window of its own. */
function openRaw(info) {
  new WebviewWindow(`raw-win-${Date.now()}-${++rawWinSeq}`, {
    url: "/?raw=" + encodeURIComponent(JSON.stringify(info)),
    title: `${info.title || "原始数据"} — 原始数据 — Smart Library`,
    width: 1000,
    height: 860,
  });
}

/** A search tab into a window of its own (not kept after a restart). */
async function popOutSearch(id) {
  const st = searchStates[id];
  if (!st) return;
  const info = searchSnapshot(st);
  dv.getPanel(id)?.api.close();
  scheduleSave();
  new WebviewWindow(`search-win-${Date.now()}-${++searchWinSeq}`, {
    url: "/?search=" + encodeURIComponent(JSON.stringify(info)),
    title: "全文搜索 — Smart Library",
    width: 1100,
    height: 820,
  });
}

/** A standalone AI chat tab into a window of its own. */
export async function popOutAiChat(id) {
  const st = aiChatStates[id];
  const info = { sessionId: st?.sessionId || null, title: st?.title || "AI 助手" };
  dv?.getPanel(id)?.api.close();
  scheduleSave();
  new WebviewWindow(`aichat-win-${Date.now()}-${++aiChatWinSeq}`, {
    url: "/?aichat=" + encodeURIComponent(JSON.stringify(info)),
    title: `${info.title} — Smart Library`,
    width: 760,
    height: 820,
  });
}

function listenToPopouts() {
  listen("popout-progress", (e) => recordProgress(e.payload.tab, e.payload.lb, e.payload.mode));
  listen("popout-closed", (e) => {
    const w = popouts[e.payload.tab];
    delete popouts[e.payload.tab];
    scheduleSave();
    if (w) {
      api.recentBump(w.lib, w.path).then(() => emit("data-changed").catch(() => {})).catch(() => {});
    }
  });
  listen("popout-dock", (e) => {
    const q = e.payload;
    delete popouts[q.tab];
    openVolume({ lib: q.lib, bookId: q.bookId, path: q.path, lb: q.lb });
  });
  listen("popout-open", async (e) => {
    // from a reader or search window: the document opens here, so show this window
    const w = getCurrentWindow();
    await w.unminimize().catch(() => {});
    await w.setFocus().catch(() => {});
    openVolume(e.payload).catch((err) => hooks.onaction?.({ type: "error", text: String(err) }));
  });
  listen("search-dock", (e) => openSearch({ ...e.payload, autorun: !!e.payload.ran, fresh: true }));
  listen("aichat-dock", async (e) => {
    const w = getCurrentWindow();
    await w.unminimize().catch(() => {});
    await w.setFocus().catch(() => {});
    if (hooks.onaidock) hooks.onaidock(e.payload || {});
    else openAiChat({ sessionId: e.payload?.sessionId || null, title: e.payload?.title || "AI 助手" });
  });
}

// ------------------------------------------------------------ public

export function setLibraries(libs) {
  hooks.libraries = libs || [];
}

/** Create the dockview in `element` and restore the saved workspace. */
export async function init(element, h) {
  hooks = h;
  hostEl = element;
  dv = createDockview(element, {
    className: "dockview-theme-light sl-dock",
    createComponent: (opts) =>
      opts.name === "search"
        ? searchRenderer()
        : opts.name === "aichat"
          ? aiChatRenderer()
          : opts.name === "folder"
            ? folderRenderer()
            : opts.name === "results"
              ? oldResultsRenderer()
              : readerRenderer(),
    createWatermarkComponent: watermark,
    getTabContextMenuItems: tabMenu,
  });
  dv.onDidLayoutChange(() => {
    scheduleSave();
    for (const id of Object.keys(tooltips)) applyTooltip(id);
  });
  const refreshEmpty = () => (ws.empty = dv.panels.length === 0);
  // the document being read = the last active reader (the results tab and
  // an empty workspace keep / clear it)
  dv.onDidActivePanelChange((e) => {
    const panel = e?.panel; // the event is { panel, origin }; panel is undefined when nothing is active
    // Ignore Dockview's intermediate auto-activation while we are redirecting
    // a closed active tab back to the previous MRU tab.
    if (pendingCloseTarget && panel?.id !== pendingCloseTarget) {
      return;
    }
    if (panel?.id === pendingCloseTarget) {
      pendingCloseTarget = null;
    }
    lastActiveId = panel?.id ?? null;
    if (!restoring && panel) touchActivation(panel.id);
    if (panel && isSearchTab(panel.id)) lastSearchId = panel.id;
    recordViewHistory(panel);
    const isReaderPanel = panel && !isSearchTab(panel.id) && !isAiChatTab(panel.id) && !isFolderTab(panel.id) && panel.id !== "search";
    // a restored tab that was never shown has no document info yet: its parameters have it
    const i = isReaderPanel
      ? infos[panel.id] || (panel.params?.lib ? { title: panel.title, lib: panel.params.lib, path: panel.params.path, bookId: panel.params.bookId ?? null, author: "" } : null)
      : null;
    if (i) ws.current = { ...i, title: panel.title || i.title };
    else if (!panel || !dv.panels.some((x) => infos[x.id])) ws.current = null;

    if (panel && isSearchTab(panel.id)) {
      ws.activeTab = { kind: "search", title: panel.title || "全文搜索" };
      ws.activeDoc = null;
      ws.activeSearch = searchResults[panel.id] || null;
    } else if (panel && isAiChatTab(panel.id)) {
      ws.activeTab = { kind: "aichat", title: panel.title || "AI 助手" };
      ws.activeDoc = null;
      ws.activeSearch = null;
    } else if (panel && isFolderTab(panel.id)) {
      ws.activeTab = { kind: "folder", title: panel.title || "目录" };
      ws.activeDoc = null;
      ws.activeSearch = null;
    } else if (panel && i) {
      ws.activeTab = { kind: "doc" };
      ws.activeDoc = docStates[panel.id] || null;
      ws.activeSearch = null;
    } else {
      ws.activeTab = panel ? { kind: "other", title: panel.title || "" } : null;
      ws.activeDoc = null;
      ws.activeSearch = null;
    }
    if (panel && states[panel.id]) {
      states[panel.id].check = (states[panel.id].check ?? 0) + 1;
    }
    requestAnimationFrame(() => revealActiveTab(panel));
  });
  dv.onDidAddPanel(refreshEmpty);
  dv.onDidRemovePanel((panel) => {
    const wasActive = panel?.id != null && panel.id === lastActiveId;
    const closedGroupId = panel?.group?.id ?? null;
    if (panel?.id) dropActivation(panel.id);
    if (wasActive) lastActiveId = null;

    refreshEmpty();
    if (ws.current && panel.id === `${ws.current.lib}|${ws.current.path}`) ws.current = null;
    if (dv.panels.length === 0) {
      pendingCloseTarget = null;
      ws.activeTab = null;
      ws.activeDoc = null;
      ws.activeSearch = null;
    } else if (!restoring && wasActive) {
      const next = pickNextActiveAfterClose(closedGroupId);
      if (next) {
        const targetId = next.id;
        pendingCloseTarget = targetId;
        queueMicrotask(() => {
          const target = dv?.getPanel(targetId);
          if (!target) {
            if (pendingCloseTarget === targetId) pendingCloseTarget = null;
            return;
          }
          if (dv.activePanel?.id === targetId) {
            if (pendingCloseTarget === targetId) pendingCloseTarget = null;
            lastActiveId = targetId;
            touchActivation(targetId);
            return;
          }
          target.api.setActive();
        });
      }
    }
    if (!restoring && !isFolderTab(panel?.id) && panel?.params?.lib && panel?.params?.path != null) {
      const { lib, path } = panel.params;
      const tabId = panel.id;
      queueMicrotask(() => {
        if (!popouts[tabId]) {
          api.recentBump(lib, path).then(() => emit("data-changed").catch(() => {})).catch(() => {});
        }
      });
    }
  });
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
    if (Array.isArray(saved.viewHistory)) {
      viewHistory = saved.viewHistory
        .filter((x) => x && typeof x.lib === "string" && known.has(x.lib) && (x.kind === "folder" || x.path != null))
        .map((x) =>
          x.kind === "folder"
            ? {
                kind: "folder",
                tab: folderHistoryKey(x.lib, x.rel || "", x.key || ""),
                lib: x.lib,
                rel: x.rel || "",
                key: x.key || "",
                title: x.title || "",
              }
            : { tab: `${x.lib}|${x.path}`, lib: x.lib, bookId: x.bookId ?? null, path: x.path, title: x.title || "" },
        )
        .slice(-MAX_VIEW_HISTORY);
      const idx = Number.isInteger(saved.viewIndex) ? saved.viewIndex : viewHistory.length - 1;
      viewIndex = viewHistory.length ? Math.max(0, Math.min(idx, viewHistory.length - 1)) : -1;
    }
    restoring = true;
    try {
      if (saved.layout) dv.fromJSON(saved.layout);
    } catch (e) {
      console.warn("restore layout", e);
      dv.clear();
    }
    // the old single results tab is gone; tabs of unbound libraries are dropped
    dv.getPanel("search")?.api.close();
    for (const panel of dv.panels) {
      if (isSearchTab(panel.id)) searchSeq = Math.max(searchSeq, Number(panel.id.slice(7)) || 0);
      if (isAiChatTab(panel.id)) aiChatSeq = Math.max(aiChatSeq, Number(panel.id.slice(7)) || 0);
      if (isFolderTab(panel.id) && panel.id !== FOLDER_BROWSE_ID) folderSeq = Math.max(folderSeq, Number(panel.id.slice(7)) || 0);
    }
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
  syncHistoryButtons();
  if (dv.activePanel) {
    lastActiveId = dv.activePanel.id;
    touchActivation(dv.activePanel.id);
    recordViewHistory(dv.activePanel);
  }
}

/**
 * Open a document: focus its tab (or pop-out window) if it is open, else
 * open a new tab in the active group. CBETA: `bookId` (+ `path` of a
 * volume); HTML libraries: `path` of the article. `lb` jumps to a line or
 * paragraph; `query`/`phrase` highlight the hits.
 */
export async function openVolume({ lib, bookId = null, path = null, lb = null, query = null, phrase = false }) {
  const id = await api.resolveNode(lib, bookId, path);
  api.recentTouch(id.lib, id.path, id.book_id, id.title).then(() => {
      ws.opened++;
      emit("data-changed").catch(() => {});
    }).catch(() => {});
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
    if (states[id.tab]) states[id.tab].check = (states[id.tab].check ?? 0) + 1;
    return;
  }
  dv.addPanel({
    id: id.tab,
    component: "reader",
    title: id.title,
    params: { lib: id.lib, bookId: id.book_id, path: id.path, target: lb, query, phrase },
  });
}

/**
 * Open a folder / library root in a directory card tab. By default reuses the
 * active folder tab (or the shared `folder:browse` tab) so browsing folders
 * does not flood the tab strip, while leaving standard document tabs untouched.
 * Pass `newTab: true` (e.g. Ctrl/middle-click) to open an independent folder tab.
 */
export function openFolder({ lib, rel = "", key = "", title = "" }, { newTab = false, panelId = null, recordHistory = true } = {}) {
  if (!dv || !lib) return null;
  const libObj = (hooks.libraries || []).find((l) => l.id === lib);
  const label = title || (rel ? rel.split("/").filter(Boolean).pop() : "") || libObj?.title || lib;
  let target = null;
  if (!newTab) {
    if (panelId && isFolderTab(panelId)) target = dv.getPanel(panelId);
    if (!target && isFolderTab(dv.activePanel?.id)) target = dv.activePanel;
    if (!target) target = dv.getPanel(FOLDER_BROWSE_ID);
  }
  const params = { kind: "folder", lib, rel: rel || "", key: key || "", title: label };
  if (target) {
    target.api.setActive();
    target.api.updateParameters(params);
    target.api.setTitle(label);
    if (recordHistory) recordFolderHistory({ lib, rel: rel || "", key: key || "", title: label });
    return target.id;
  }
  const id = !newTab && !dv.getPanel(FOLDER_BROWSE_ID) ? FOLDER_BROWSE_ID : `folder:${++folderSeq}`;
  dv.addPanel({
    id,
    component: "folder",
    title: label,
    params,
  });
  if (recordHistory) recordFolderHistory({ lib, rel: rel || "", key: key || "", title: label });
  return id;
}

/** Reload all open folder tabs belonging to `lib` (e.g. after files change). */
export function refreshFolderTabs(lib) {
  for (const st of Object.values(folderStates)) {
    if (!lib || st.lib === lib) st.reload += 1;
  }
}

/**
 * Add text to a note that is being edited in a tab: it goes to the editor
 * (saving the editor later must not lose it). False when it is not edited.
 */
/** Close the tab being read. */
export function closeActive() {
  dv?.activePanel?.api.close();
}

/** The next (1) or previous (-1) tab, round the list. */
export function cycleTab(delta) {
  const list = dv?.panels ?? [];
  if (list.length < 2) return;
  const i = list.indexOf(dv.activePanel);
  list[(i + delta + list.length) % list.length].api.setActive();
}

function recordFolderHistory({ lib, rel = "", key = "", title = "" }) {
  if (restoring || navigatingHistory || !lib) return;
  const tab = folderHistoryKey(lib, rel, key);
  if (viewIndex >= 0 && viewHistory[viewIndex]?.tab === tab) return;
  viewHistory = viewHistory.slice(0, viewIndex + 1);
  viewHistory.push({ kind: "folder", tab, lib, rel, key, title });
  if (viewHistory.length > MAX_VIEW_HISTORY) {
    viewHistory.shift();
  }
  viewIndex = viewHistory.length - 1;
  syncHistoryButtons();
  scheduleSave();
}

function recordViewHistory(panel) {
  if (restoring || navigatingHistory || !panel) return;
  if (isFolderTab(panel.id)) {
    const st = folderStates[panel.id] || panel.params || {};
    if (st.lib) recordFolderHistory({ lib: st.lib, rel: st.rel || "", key: st.key || "", title: panel.title || st.title || "" });
    return;
  }
  const lib = panel?.params?.lib;
  const path = panel?.params?.path;
  if (!lib || path == null) return;
  const tab = `${lib}|${path}`;
  if (viewIndex >= 0 && viewHistory[viewIndex]?.tab === tab) return;
  viewHistory = viewHistory.slice(0, viewIndex + 1);
  viewHistory.push({ tab, lib, bookId: panel.params.bookId ?? null, path, title: panel.title || infos[tab]?.title || "" });
  if (viewHistory.length > MAX_VIEW_HISTORY) {
    viewHistory.shift();
  }
  viewIndex = viewHistory.length - 1;
  syncHistoryButtons();
  scheduleSave();
}

/** Enable the back / forward arrows of the sidebar's view navigation. */
function syncHistoryButtons() {
  ws.canBack = viewHistory.length > 0 && viewIndex > 0;
  ws.canForward = viewIndex >= 0 && viewIndex < viewHistory.length - 1;
}

async function activateHistoryEntry(entry) {
  if (entry.kind === "folder") {
    const exists = (hooks.libraries || []).some((l) => l.id === entry.lib);
    if (!exists) return false;
    openFolder(
      { lib: entry.lib, rel: entry.rel || "", key: entry.key || "", title: entry.title || "" },
      { newTab: false, recordHistory: false },
    );
    return true;
  }
  if (popouts[entry.tab]) {
    const w = await WebviewWindow.getByLabel(popouts[entry.tab].label);
    if (w) {
      await w.unminimize().catch(() => {});
      await w.setFocus();
      return true;
    }
    delete popouts[entry.tab];
  }
  const panel = dv?.getPanel(entry.tab);
  if (panel) {
    panel.api.setActive();
    return true;
  }
  try {
    await api.openNode(entry.lib, entry.bookId, entry.path);
    await openVolume({ lib: entry.lib, bookId: entry.bookId, path: entry.path });
    return true;
  } catch {
    return false;
  }
}

/** Navigate document view history: `-1` for previous view, `+1` for next view. */
export async function navigateHistory(delta) {
  if (!dv || navigatingHistory || !viewHistory.length) return false;
  navigatingHistory = true;
  try {
    let idx = viewIndex + delta;
    while (idx >= 0 && idx < viewHistory.length) {
      const entry = viewHistory[idx];
      if (await activateHistoryEntry(entry)) {
        viewIndex = idx;
        scheduleSave();
        return true;
      }
      viewHistory.splice(idx, 1);
      scheduleSave();
      if (delta < 0) {
        if (viewIndex > idx) viewIndex -= 1;
        idx -= 1;
      }
    }
    return false;
  } finally {
    navigatingHistory = false;
    syncHistoryButtons();
  }
}

/**
 * The view history for the "视图历史" dialog, newest first: what each entry
 * is, and which one is the view being read now (`index` is the position in
 * the history, so a row can be picked directly).
 */
export function viewHistorySnapshot() {
  const items = viewHistory.map((e, index) => {
    const libTitle = (hooks.libraries || []).find((l) => l.id === e.lib)?.title || e.lib;
    return {
      index,
      title: e.title || infos[e.tab]?.title || e.path || e.rel || e.bookId || libTitle || e.tab,
      lib: e.lib,
      libTitle,
      path: e.kind === "folder" ? (e.rel || e.key || "") : e.path,
      bookId: e.bookId ?? null,
      isFolder: e.kind === "folder",
    };
  });
  return { index: viewIndex, items: items.reverse() };
}

/**
 * Go straight to one entry of the view history (a row of the dialog). Unlike
 * `navigateHistory` there are no entries in between; an entry that cannot be
 * opened any more is dropped, as when stepping past it.
 */
export async function navigateHistoryTo(index) {
  if (!dv || navigatingHistory || index < 0 || index >= viewHistory.length) return false;
  navigatingHistory = true;
  try {
    if (await activateHistoryEntry(viewHistory[index])) {
      viewIndex = index;
      scheduleSave();
      return true;
    }
    viewHistory.splice(index, 1);
    if (viewIndex >= index) viewIndex -= 1;
    scheduleSave();
    return false;
  } finally {
    navigatingHistory = false;
    syncHistoryButtons();
  }
}

/** Forget the view history (the back / forward arrows become unavailable). */
export function clearViewHistory() {
  viewHistory = [];
  viewIndex = -1;
  syncHistoryButtons();
  scheduleSave();
}

/** The line the active tab was last read at (null: none known). */
export function currentLine() {
  const c = ws.current;
  return (c && progress[`${c.lib}|${c.path}`]?.lb) || null;
}

export function appendIfEditing(lib, id, text) {
  const st = states[`${lib}|${id}`];
  if (!st?.editing) return false;
  st.append = { text, n: ++appendSeq };
  return true;
}

/** Read an open document again (a note changed outside its tab). */
export function refreshDocument(lib, id) {
  const st = states[`${lib}|${id}`];
  if (st) st.reload += 1;
}

/** Close the tab of one document (a deleted note). */
export function closeDocument(lib, path) {
  dv?.getPanel(`${lib}|${path}`)?.api.close();
  scheduleSave();
}

/**
 * Close the tabs of a file or of the files below a folder (before it is renamed
 * or moved: their editors save while closing). Resolves to the paths that were
 * open, and the one that was active, once the saves have had time to finish.
 */
export async function closeUnder(lib, path) {
  const open = (dv?.panels || []).filter((p) => !isFolderTab(p.id) && p.params?.lib === lib && (p.params.path === path || p.params.path?.startsWith(path + "/")));
  const openFolders = (dv?.panels || []).filter((p) => {
    if (!isFolderTab(p.id)) return false;
    const st = folderStates[p.id] || p.params;
    return st?.lib === lib && (st.rel === path || st.rel?.startsWith(path + "/"));
  });
  const active = ws.current?.lib === lib ? ws.current.path : null;
  const paths = open.map((p) => p.params.path);
  open.forEach((p) => p.api.close());
  openFolders.forEach((p) => p.api.close());
  if (open.length || openFolders.length) {
    scheduleSave();
    if (open.length) await new Promise((r) => setTimeout(r, 400));
  }
  return { paths, active: active && paths.includes(active) ? active : null };
}

/**
 * Close the reading tabs of a library, run `change` (a change of its mode: the
 * readers differ), and open the same documents again (the active one last).
 */
export async function reopenLibrary(lib, change) {
  const open = (dv?.panels || []).filter((p) => p.params?.lib === lib && p.params.path != null);
  const active = ws.current?.lib === lib ? ws.current.path : null;
  const paths = open.map((p) => p.params.path);
  open.forEach((p) => p.api.close()); // their editors save while closing
  if (open.length) {
    scheduleSave();
    await new Promise((r) => setTimeout(r, 400));
  }
  try {
    await change();
  } finally {
    for (const path of paths.filter((x) => x !== active)) await openVolume({ lib, path }).catch(() => {});
    if (active) await openVolume({ lib, path: active }).catch(() => {});
  }
}

/** Show a new title on the tab of a document (a renamed note). */
export function retitle(lib, path, title) {
  dv?.getPanel(`${lib}|${path}`)?.api.setTitle(title);
  const st = states[`${lib}|${path}`];
  if (st) st.retitled = title; // the title in the tab's own tool bar
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

/**
 * Open a search page in a new tab (several can be open). `query` / `phrase` /
 * `scopes` fill it in. The page searches at once (an empty query lists
 * everything); a page that already shows everything is used again.
 */
export function openSearch({ query = "", phrase = false, scopes = [], autorun = false, fresh = false, ...more } = {}) {
  const blank = !query && !scopes.length && !fresh;
  if (blank) {
    for (const [id, st] of Object.entries(searchStates)) {
      if (st.ran && !st.query.trim() && !st.scopes.length && !Object.values(st.filters).some((v) => v.length)) {
        dv.getPanel(id)?.api.setActive();
        st.focus += 1;
        return id;
      }
    }
  }
  const id = `search:${++searchSeq}`;
  dv.addPanel({
    id,
    component: "search",
    title: "全文搜索",
    params: { kind: "search", ...more, query, phrase, scopes, autorun: autorun || !fresh },
  });
  return id;
}

/** Add nodes to the search range of the search page used last (a new page when there is none). */
export function addSearchScopes(items) {
  const st = lastSearchId && searchStates[lastSearchId];
  if (!st) return openSearch({ scopes: items });
  const have = new Set(st.scopes.map((x) => `${x.lib}|${x.label}`));
  const fresh = items.filter((x) => !have.has(`${x.lib}|${x.label}`));
  if (fresh.length) {
    st.scopes = [...st.scopes, ...fresh];
    st.filters = {};
    st.tab = "scope";
    st.rerun += 1;
  }
  dv.getPanel(lastSearchId)?.api.setActive();
  return lastSearchId;
}

/** Open a standalone AI chat tab in the workspace. */
export function openAiChat({ sessionId = null, title = "AI 助手" } = {}) {
  const id = `aichat:${++aiChatSeq}`;
  dv.addPanel({
    id,
    component: "aichat",
    title,
    params: { kind: "aichat", sessionId },
  });
  return id;
}

/** Open a standalone AI chat window directly. */
export function openAiWindow({ sessionId = null, title = "AI 助手" } = {}) {
  const info = { sessionId, title };
  new WebviewWindow(`aichat-win-${Date.now()}-${++aiChatWinSeq}`, {
    url: "/?aichat=" + encodeURIComponent(JSON.stringify(info)),
    title: `${title} — Smart Library`,
    width: 760,
    height: 820,
  });
}

/** Save now (e.g. before the window closes). */
export function flush() {
  clearTimeout(saveTimer);
  return save();
}
