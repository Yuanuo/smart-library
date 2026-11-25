<script>
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api.js";
  import * as workspace from "./lib/workspace.svelte.js";
  import LibraryPanel from "./lib/LibraryPanel.svelte";
  import BindDialog from "./lib/BindDialog.svelte";
  import QuickSearch from "./lib/QuickSearch.svelte";
  import { openDictWindow } from "./lib/dictWindow.js";
  import Icon from "./lib/Icon.svelte";
  import RecentDialog from "./lib/RecentDialog.svelte";
  import ViewHistoryDialog from "./lib/ViewHistoryDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import TagDialog from "./lib/TagDialog.svelte";
  import MarksDialog from "./lib/MarksDialog.svelte";
  import PropsDialog from "./lib/PropsDialog.svelte";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import UpdateDialog from "./lib/UpdateDialog.svelte";
  import PinyinDialog from "./lib/PinyinDialog.svelte";
  import { checkUpdates } from "./lib/updater.svelte.js";
  import { installShortcuts, readerSelection, formatShortcut } from "./lib/shortcuts.js";
  import { localDate, localTime } from "./lib/diary.js";
  import { settings, update as updateSettings, activeAiPreset, isAiConfigured } from "./lib/settings.svelte.js";
  import { daily, check as checkDaily, again as dailyAgain, markSeen as dailySeen } from "./lib/daily.svelte.js";
  import { status } from "./lib/status.svelte.js";
  import { openPanelWindow } from "./lib/panels.js";
  import { fullPath } from "./lib/paths.js";
  import ClipDialog from "./lib/ClipDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import UnbindRemoteDialog from "./lib/UnbindRemoteDialog.svelte";
  import AiPanel from "./lib/AiPanel.svelte";
  import { open as openFile } from "@tauri-apps/plugin-dialog";
  import { clipMarkdown, clipNode } from "./lib/clip.js";
  import { getLast, setLast } from "./lib/clipTarget.js";
  import { emit, listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let st = $state(null);
  let error = $state("");
  let pinyinDlg = $state("");
  let notice = $state("");

  // Messages appear as a toast over the window (the layout never moves) and
  // are kept in a list for the rest of the run. A toast goes away after a few
  // seconds, but stays while the mouse rests on it or nothing was touched
  // since it appeared (the user may be away).
  let log = $state([]); // { id, time, kind, text }, newest first
  let unreadLog = $state(0);
  let toast = $state(null);
  let showLog = $state(false);
  let lastActive = Date.now();
  let toastTimer = null;
  let toastSeq = 0;
  let toastHover = false;

  function toggleLog() {
    showLog = !showLog;
    if (showLog) unreadLog = 0;
  }

  function push(kind, text) {
    const m = { id: ++toastSeq, time: new Date().toLocaleTimeString("zh-CN", { hour12: false }), kind, text };
    log = [m, ...log].slice(0, 200);
    if (!showLog) unreadLog += 1;
    toast = m;
    scheduleHide(m.id, Date.now(), kind === "error" ? 10000 : 5000);
  }

  function scheduleHide(id, shownAt, ms) {
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      if (toast?.id !== id) return;
      if (toastHover || lastActive < shownAt) scheduleHide(id, shownAt, 2000);
      else toast = null;
    }, ms);
  }

  $effect(() => {
    if (error) {
      push("error", error);
      error = "";
    }
  });
  $effect(() => {
    if (notice) {
      push("notice", notice);
      notice = "";
    }
  });
  $effect(() => {
    const touch = () => (lastActive = Date.now());
    const evs = ["mousemove", "pointerdown", "keydown", "wheel"];
    evs.forEach((e) => window.addEventListener(e, touch, { passive: true }));
    return () => evs.forEach((e) => window.removeEventListener(e, touch));
  });

  let progress = $state({}); // lib id -> { done, total }
  let titlesProgress = $state({}); // lib id -> { done, total }: title list being built
  let binding = $state(null); // null, or { path } for the bind dialog
  let quick = $state(false); // title quick search popup

  /**
   * A shortcut (or one forwarded from a pop-out reader). With text selected
   * in a reader the searches start from it: Ctrl+G / Shift twice = title
   * quick search, Ctrl+H = full-text search.
   */
  function shortcut(type, text = "") {
    if (binding) return;
    text = text.slice(0, 200);
    if (type === "quick") text ? readerAction({ type: "quick", text }) : openQuickSearch();
    else if (type === "search") text ? readerAction({ type: "search", text }) : openSearch();
    else if (type === "dict") openDictWindow(text);
    else if (type === "recent") openRecent();
    else if (type === "marks") openMarks();
    else if (type === "settings") openSettings("appearance");
    else if (type === "ai") {
      if (!settings.aiOn) updateSettings({ aiOn: true });
      setAiOpen(!aiOpen);
    }
    else if (type === "diary") libPanel?.openDiary();
    else if (type === "new") libPanel?.newNote();
    else if (type === "new-mindmap") libPanel?.newMindMap();
    else if (type === "new-whiteboard") libPanel?.newWhiteboard();
    else if (type === "close") workspace.closeActive();
    else if (type === "next-tab") workspace.cycleTab(1);
    else if (type === "prev-tab") workspace.cycleTab(-1);
    else if (type === "nav-back") workspace.navigateHistory(-1);
    else if (type === "nav-forward") workspace.navigateHistory(1);
    else if (type === "script") updateSettings({ readerScript: settings.readerScript === "simp" ? "orig" : "simp" });
    else docShortcut(type);
  }

  /** The shortcuts about the document of the active tab. */
  function docShortcut(type) {
    const c = workspace.ws.current;
    if (!c) {
      notice = "先打开一篇文档。";
      return;
    }
    const vol = { lib: c.lib, book_id: c.bookId ?? null, path: c.path, title: c.title, author: c.author ?? "" };
    if (type === "tag") markAction({ type: "tag", vol });
    else if (type === "bookmark") markAction({ type: "bookmark", vol, lb: workspace.currentLine() });
    else if (type === "locate") locate({ lib: c.lib, bookId: c.bookId ?? null, path: c.path });
    else if (type === "props") openPropsOf(c);
  }

  let quickQuery = $state(null); // text to start the quick search with

  /** Actions of the reader's context menu that the main window carries out. */
  function readerAction(a) {
    const v = a.vol ?? {};
    if (a.type === "search" || a.type === "phrase" || a.type === "book") {
      // a new search page; "search in this book" limits it to the book
      const scopes = a.type === "book" ? [{ lib: v.lib, label: v.title, books: v.book_id ? [v.book_id] : [], paths: v.book_id ? [] : [v.path], rels: [], whole: false }] : [];
      workspace.openSearch({ query: a.text, phrase: a.type === "phrase", scopes, autorun: true });
    } else if (a.type === "note") {
      clipDlg = { text: a.text, vol: v, lb: a.lb };
    } else if (a.type === "diary") {
      sendToDiary({ text: a.text, vol: v, lb: a.lb }).catch((e) => (error = String(e)));
    } else if (a.type === "note-last") {
      const t = getLast();
      if (!t) error = "还没有最近的笔记。";
      else doClip({ mode: "append", target: t, comment: "" }, { text: a.text, vol: v, lb: a.lb }).catch((e) => (error = String(e)));
    } else if (a.type === "ai-external") {
      handleAiRequest({
        text: a.text,
        prompt: a.prompt,
        autoSend: a.autoSend,
        title: v.title || v.url,
        lb: a.lb,
        append: !!a.append,
        source: a.source || "",
      });
    } else if (a.type === "error") {
      error = a.text;
    } else if (a.type === "quick") {
      quickScope = null;
      quickQuery = a.text;
      recentBox = false;
      quick = true;
    } else if (a.type === "pinyin") {
      if (a.text) pinyinDlg = a.text;
    } else if (a.type === "dict" || a.type === "dict-search" || a.type === "dict-lookup") {
      openDictWindow(a.text || "");
    } else {
      markAction(a);
    }
  }

  async function copyPath(text) {
    try {
      await navigator.clipboard.writeText(text);
      notice = `已复制路径：${text}`;
    } catch (e) {
      error = String(e);
    }
  }

  /** "Copy path" of a reading tab: the original file, or the titles in the tree. */
  async function copyTabPath({ lib, bookId, path, title, tree }) {
    const l = libraries.find((x) => x.id === lib);
    if (tree || !l || l.kind === "notes" || !path) {
      const tp = await libPanel?.treePath({ lib, bookId, path });
      copyPath(tp ?? [l?.title, title].filter(Boolean).join(" › "));
    } else copyPath(fullPath(l, path));
  }

  // ---- export and import of libraries
  let exportDlg = $state(null); // { lib, sel, label }
  let importing = $state(false);

  /** `target`: { lib, parent, key } to import below a place of an internal library. */
  async function importLibrary(target) {
    if (importing || !target) return;
    try {
      const path = await openFile({
        multiple: false,
        title: "选择数据包",
        filters: [
          { name: "知识库数据包（*.smartlib, *.smartdir）", extensions: ["smartlib", "smartdir"] },
          { name: "文件夹数据包（*.smartdir）", extensions: ["smartdir"] },
          { name: "ZIP", extensions: ["zip"] },
        ],
      });
      if (!path) return;
      importing = true;
      notice = "正在导入…";
      const r = await api.importPackage(path, target.lib, target.parent ?? null);
      st = r.status;
      await libPanel?.reloadNode(target.lib, target.key);
      notice = `已导入到「${r.title}」：${r.notes} 篇笔记。`;
      api.prepareTitles().catch(() => {});
    } catch (e) {
      error = String(e);
    } finally {
      importing = false;
    }
  }

  // ---- clippings: selected text into a note
  /** Add the selected text to today's diary (made when missing), after the time. */
  async function sendToDiary(a) {
    const r = await api.diaryToday(localDate());
    applyLibrariesStatus(r.status);
    const md = `**${localTime()}**

` + (await clipMarkdown({ text: a.text, vol: a.vol, lb: a.lb, comment: "" }));
    const t = r.note;
    if (!workspace.appendIfEditing(t.lib, t.id, md)) {
      await api.noteAppend(t.lib, t.id, md);
      workspace.refreshDocument(t.lib, t.id);
    }
    await libPanel?.showNew(t, false);
    notice = `已追加到今日记：${localDate()}`;
    api.prepareTitles().catch(() => {});
  }

  let clipDlg = $state(null); // { text, vol, lb }

  /** Add a clipping (`a`: { text, vol, lb }) to a new or an existing note. */
  async function doClip(req, a) {
    if (req.mode === "append" && req.target.mind) {
      // a mind map: the text becomes a node under its root, linked to the source
      const t = req.target;
      const n = await clipNode({ text: a.text, vol: a.vol, lb: a.lb });
      await api.mindmapAddClip(t.lib, t.id, n.topic, n.link, n.tag);
      workspace.refreshDocument(t.lib, t.id);
      setLast(t);
      notice = `已加入脑图：${t.title}`;
      return;
    }
    const md = await clipMarkdown({ text: a.text, vol: a.vol, lb: a.lb, comment: req.comment });
    if (req.mode === "new") {
      const r = await api.createNoteAt(null, req.where, req.title);
      await api.saveNote(r.lib, r.id, md + "\n");
      await libPanel?.showNew(r, false);
      setLast({ lib: r.lib, id: r.id, title: req.title });
      notice = `已加入新笔记：${req.title}`;
    } else {
      const t = req.target;
      if (!workspace.appendIfEditing(t.lib, t.id, md)) {
        await api.noteAppend(t.lib, t.id, md);
        workspace.refreshDocument(t.lib, t.id);
      }
      setLast(t);
      notice = `已追加到笔记：${t.title}`;
    }
    api.prepareTitles().catch(() => {});
  }

  // ---- bookmarks, collections and tags
  /** Tell the windows of their own (recent, marks) that data changed. */
  const ping = () => emit("data-changed").catch(() => {});
  let tagDlg = $state(null); // props of the TagDialog
  let marksBox = $state(false);

  const docTitle = (v) => (v.label ? `${v.title} ${v.label}` : v.title);

  async function markAction(a) {
    const v = a.vol;
    try {
      if (a.type === "bookmark") {
        await api.markAdd({
          kind: "bookmark", lib: v.lib, path: v.path, bookId: v.book_id ?? null, lb: a.lb ?? null,
          quote: a.excerpt ?? "", note: "", docTitle: docTitle(v), author: v.author ?? "", tags: "",
        });
        notice = `已添加书签：${docTitle(v)}${a.lb ? " · " + a.lb : ""}`;
        ping();
      } else if (a.type === "clip") {
        tagDlg = {
          heading: "添加收藏", subtitle: `${docTitle(v)}${a.lb ? " · " + a.lb : ""}`, quote: a.text, withNote: true, okLabel: "添加",
          onok: async (tags, note) => {
            await api.markAdd({
              kind: "clip", lib: v.lib, path: v.path, bookId: v.book_id ?? null, lb: a.lb ?? null,
              quote: a.text, note, docTitle: docTitle(v), author: v.author ?? "", tags,
            });
            notice = "已添加收藏。";
            ping();
          },
        };
      } else if (a.type === "tag") {
        const l = libraries.find((x) => x.id === v.lib);
        const bookId = v.book_id ?? null;
        const nodePath = libPanel?.nodePath(v.lib, { bookId, path: v.path }) || [l?.title, docTitle(v)].filter(Boolean).join("/");
        tagNode(v.lib, bookId, v.path, docTitle(v), nodePath);
      } else if (a.type === "props") {
        openPropsOf({ lib: v.lib, bookId: v.book_id ?? null, path: v.path, title: docTitle(v) });
      }
    } catch (e) {
      error = String(e);
    }
  }

  /** Tags of a whole book / article. */
  async function tagNode(lib, bookId, path, title, nodePath = "") {
    const current = await api.nodeTagsGet(lib, bookId, path);
    tagDlg = {
      heading: `标签：${title}`, nodePath, subtitle: "", tags: current.join(", "),
      onok: async (tags) => {
        await api.nodeTagsSet(lib, bookId, path, title, tags);
        notice = tags.trim() ? "标签已保存。" : "已清除标签。";
        ping();
      },
    };
  }

  let propsDlg = $state(null); // { lib, libTitle, bookId, path, title, nodePath }
  function openPropsOf(c) {
    const l = libraries.find((x) => x.id === c.lib);
    if (!l) return;
    const bookId = l.kind === "cbeta" ? c.bookId : null;
    const path = l.kind === "cbeta" ? "" : c.path;
    const nodePath = libPanel?.nodePath(l.id, { bookId, path }) || [l.title, c.title].filter(Boolean).join("/");
    propsDlg = { lib: l.id, libTitle: l.title, bookId, path, title: c.title, nodePath };
  }
  /** The properties dialog of a node: CBETA the whole book; the others the article / file / folder / note. */
  function propsFromTree(l, node, treeKey = "", nodePath = "") {
    if (!node) {
      propsDlg = { lib: l.id, libTitle: l.title, bookId: null, path: "", title: l.title, nodePath: l.title };
      return;
    }
    const bookId = l.kind === "cbeta" ? node.book_id : null;
    const isNavFolder = (l.kind === "cbeta" || l.kind === "html") && !node.book_id && !node.path;
    const path = isNavFolder ? `@nav${treeKey}` : l.kind === "cbeta" ? "" : (node.path ?? node.rel);
    if (path == null) return;
    propsDlg = { lib: l.id, libTitle: l.title, bookId, path, title: node.title, nodePath: nodePath || l.title };
  }

  function tagFromTree(l, node, nodePath = "") {
    // CBETA: the whole book; the others: the article / file / folder / note
    if (l.kind === "cbeta") {
      if (!node.book_id) return;
      tagNode(l.id, node.book_id, "", node.title, nodePath).catch((e) => (error = String(e)));
    } else {
      tagNode(l.id, null, node.path ?? node.rel, node.title, nodePath).catch((e) => (error = String(e)));
    }
  }

  /** Open a mark: at its place, the quote highlighted. */
  async function openMark(m) {
    try {
      const quote = m.kind === "clip" && m.quote && m.quote.length <= 80 ? m.quote : null;
      await workspace.openVolume({
        lib: m.lib, bookId: m.book_id, path: m.path || null, lb: m.lb, query: quote, phrase: !!quote,
      });
    } catch (e) {
      error = String(e);
    }
  }

  let quickScope = $state(null);
  /** A node of the tree as an item of a search range. */
  function rangeItem({ lib, label, within }) {
    return {
      lib,
      label,
      books: within?.books ?? [],
      paths: within?.paths ?? [],
      rels: within?.rels ?? (within?.rel ? [within.rel] : []),
      whole: !within,
    };
  }
  /** From the tree: title quick search, a new full-text search page, or add to the range of the page used last. */
  function searchFrom(kind, scope) {
    if (kind === "title") {
      quickScope = scope;
      openQuickSearch(scope);
    } else if (kind === "scope-add") workspace.addSearchScopes([rangeItem(scope)]);
    else if (kind === "daily-add") {
      const it = rangeItem(scope);
      const have = settings.dailySources.some((x) => x.lib === it.lib && x.label === it.label);
      if (!have) updateSettings({ dailySources: [...settings.dailySources, it] });
      notice = have ? `已经是每日一句的来源：${it.label}` : `已加入每日一句来源：${it.label}`;
    }
    else openSearch(scope);
  }
  function openMarks() {
    quick = false;
    recentBox = false;
    marksBox = true;
  }
  let settingsBox = $state(false);
  let settingsTab = $state("appearance");
  function openSettings(tab = "appearance") {
    settingsTab = tab || "appearance";
    settingsBox = true;
  }
  let mcpState = $state({ running: false, url: "http://127.0.0.1:27123/mcp", error: "" });
  const aiConfigured = $derived(isAiConfigured(activeAiPreset()));
  async function refreshMcpStatus() {
    try {
      const r = await api.mcpStatusGet();
      mcpState = {
        running: !!r?.running,
        url: r?.url || `http://127.0.0.1:${r?.config?.port || 27123}/mcp`,
        error: r?.error || "",
      };
    } catch {}
  }
  let recentBox = $state(false); // reading history popup
  function openRecent() {
    quick = false;
    recentBox = true;
  }
  let viewHistoryBox = $state(false); // the back / forward arrows' history
  /** Right-click on a view arrow: the history dialog (a menu has no room for a long history). */
  function openViewHistory(e) {
    e.preventDefault();
    viewHistoryBox = true;
  }
  async function openRecentItem(it) {
    try {
      workspace.seedProgress(it.lib, it.path, it.lb, it.mode);
      await workspace.openVolume({ lib: it.lib, bookId: it.book_id, path: it.path });
    } catch (e) {
      error = String(e);
    }
  }
  function openQuickSearch(scope = null) {
    recentBox = false;
    quickScope = scope && scope.lib ? scope : null; // a click passes the event
    quickQuery = null;
    quick = true;
  }
  /** A full-text search page (a click on the button passes the event). */
  function openSearch(scope = null) {
    quick = false;
    workspace.openSearch({ scopes: scope && scope.lib ? [rangeItem(scope)] : [] });
  }

  // window title: the active tab (document with author & library, search, or AI chat),
  // then the program and its version; just the program when nothing is open
  let appInfo = $state(null);
  $effect(() => {
    const tab = workspace.ws.activeTab;
    const prog = `Smart Library${appInfo ? " " + appInfo.version : ""}`;
    let title = prog;
    if (tab?.kind === "doc") {
      const c = workspace.ws.current;
      const lib = c ? libraries.find((l) => l.id === c.lib)?.title : "";
      title = c
        ? [`${c.title}${c.author ? " by " + c.author : ""}`, lib ? `『${lib}』` : "", prog].filter(Boolean).join(" - ")
        : prog;
    } else if (tab?.title) {
      title = `${tab.title} - ${prog}`;
    }
    document.title = title;
    getCurrentWindow().setTitle(title).catch(() => {});
  });

  // the dialogs that can live in a window of their own
  async function popoutPanel(kind) {
    recentBox = false;
    marksBox = false;
    try {
      await openPanelWindow(kind);
    } catch (e) {
      error = String(e);
    }
  }
  function sendDoc() {
    const c = workspace.ws.current;
    emit("current-doc", c ? { ...c } : null).catch(() => {});
  }
  $effect(() => {
    workspace.ws.current;
    sendDoc();
  });
  let aboutBox = $state(false);

  let wsEl = $state();
  let libPanel = $state();

  // ---- unified right AI assistant sidebar
  const AI_MIN = 280;
  let aiOpen = $state(false);
  try {
    aiOpen = localStorage.getItem("ai.open") === "1";
  } catch {}
  function setAiOpen(v) {
    aiOpen = !!v;
    try {
      localStorage.setItem("ai.open", aiOpen ? "1" : "0");
    } catch {}
  }
  let aiWidth = $state(380);
  try {
    aiWidth = Math.max(AI_MIN, Number(localStorage.getItem("ai.width")) || 380);
  } catch {}
  let aiDragging = $state(false);
  let aiSeq = 0;
  let aiIncoming = $state(null);
  let aiSessionId = $state(null);

  function handleAiRequest(req = {}) {
    if (!settings.aiOn) return;
    if (req.toggle && !req.text && !req.prompt) {
      setAiOpen(!aiOpen);
      return;
    }
    setAiOpen(true);
    if (req.text || req.prompt) {
      const source = req.source || [req.sourceDoc?.title || req.title, req.lb].filter(Boolean).join(" · ");
      aiIncoming = {
        nonce: ++aiSeq,
        text: req.text || "",
        prompt: req.prompt || "",
        sourceDoc: req.sourceDoc || null,
        source,
        append: !!req.append,
      };
    }
  }

  function startAiDrag(e) {
    e.preventDefault();
    aiDragging = true;
    const x0 = e.clientX;
    const w0 = aiWidth;
    const moveTo = (ev) => {
      aiWidth = Math.round(Math.min(Math.max(AI_MIN, w0 - (ev.clientX - x0)), window.innerWidth * 0.65));
    };
    const stop = () => {
      aiDragging = false;
      window.removeEventListener("pointermove", moveTo);
      window.removeEventListener("pointerup", stop);
      try {
        localStorage.setItem("ai.width", String(aiWidth));
      } catch {}
    };
    window.addEventListener("pointermove", moveTo);
    window.addEventListener("pointerup", stop);
  }
  function aiSplitterKey(e) {
    const d = e.key === "ArrowLeft" ? 20 : e.key === "ArrowRight" ? -20 : 0;
    if (!d) return;
    e.preventDefault();
    aiWidth = Math.max(AI_MIN, aiWidth + d);
    try {
      localStorage.setItem("ai.width", String(aiWidth));
    } catch {}
  }

  // width and visibility of the tree panel: kept in local storage
  let sideOpen = $state(true);
  try {
    sideOpen = localStorage.getItem("side.open") !== "0";
  } catch {}
  function setSideOpen(v) {
    sideOpen = !!v;
    try {
      localStorage.setItem("side.open", sideOpen ? "1" : "0");
    } catch {}
  }

  const SIDE_MIN = 180;
  let sideWidth = $state(300);
  try {
    sideWidth = Math.max(SIDE_MIN, Number(localStorage.getItem("side.width")) || 300);
  } catch {}
  let dragging = $state(false);
  function startDrag(e) {
    e.preventDefault();
    dragging = true;
    const x0 = e.clientX;
    const w0 = sideWidth;
    const moveTo = (ev) => {
      sideWidth = Math.round(Math.min(Math.max(SIDE_MIN, w0 + ev.clientX - x0), window.innerWidth * 0.7));
    };
    const stop = () => {
      dragging = false;
      window.removeEventListener("pointermove", moveTo);
      window.removeEventListener("pointerup", stop);
      try {
        localStorage.setItem("side.width", String(sideWidth));
      } catch {}
    };
    window.addEventListener("pointermove", moveTo);
    window.addEventListener("pointerup", stop);
  }
  function splitterKey(e) {
    const d = e.key === "ArrowLeft" ? -20 : e.key === "ArrowRight" ? 20 : 0;
    if (!d) return;
    e.preventDefault();
    sideWidth = Math.max(SIDE_MIN, sideWidth + d);
    try {
      localStorage.setItem("side.width", String(sideWidth));
    } catch {}
  }

  async function locate(doc) {
    if (!sideOpen) setSideOpen(true);
    const ok = await libPanel?.reveal(doc);
    if (!ok) notice = "目录树里没有找到这个文档。";
  }
  let wsReady = false;

  const libraries = $derived(st?.libraries ?? []);
  // Active background tasks (index building, title indexing, git pull, status tasks).
  // Shown via a circular progress indicator on the left rail + a dismissible progress popup.
  const activeTasks = $derived.by(() => {
    const list = [];
    for (const id of Object.keys(progress)) {
      const p = progress[id];
      const title = libraries.find((l) => l.id === id)?.title ?? id;
      const pct = Math.min(100, Math.floor((100 * (p.done || 0)) / Math.max(1, p.total || 1)));
      list.push({ id: `idx:${id}`, title: `${title} · 建立全文索引`, detail: `${p.done || 0} / ${p.total || 1}`, pct });
    }
    for (const id of Object.keys(titlesProgress)) {
      const p = titlesProgress[id];
      const title = libraries.find((l) => l.id === id)?.title ?? id;
      const pct = Math.min(100, Math.floor((100 * (p.done || 0)) / Math.max(1, p.total || 1)));
      list.push({ id: `ttl:${id}`, title: `${title} · 更新标题列表`, detail: `${p.done || 0} / ${p.total || 1}`, pct });
    }
    for (const id of Object.keys(gitUpdating)) {
      const title = libraries.find((l) => l.id === id)?.title ?? id;
      list.push({ id: `git:${id}`, title: `${title} · 拉取远程更新`, detail: "同步中…", pct: null });
    }
    if (status.text) {
      list.push({ id: "status", title: status.text, detail: "处理中…", pct: null });
    }
    return list;
  });
  const taskOverallPct = $derived.by(() => {
    const known = activeTasks.filter((t) => t.pct != null);
    if (!known.length) return null;
    return Math.round(known.reduce((s, t) => s + t.pct, 0) / known.length);
  });
  let showTasks = $state(false);
  let tasksDismissed = $state(false);
  $effect(() => {
    if (!activeTasks.length) {
      showTasks = false;
      tasksDismissed = false;
    } else if (!tasksDismissed) {
      showTasks = true;
    }
  });
  const RING_R = 7.5;
  const RING_C = 2 * Math.PI * RING_R;

  // the full-text search pages are tabs of the workspace (lib/SearchPage.svelte)

  // ---- sentence of the day (lib/daily.svelte.js): checked on a timer, since the program stays open
  let dailyShown = $state(false);
  let dailyHover = false;
  let dailyTimer = null;
  function showDaily() {
    dailyShown = true;
    dailySeen();
    clearTimeout(dailyTimer);
    const hide = () => (dailyHover ? (dailyTimer = setTimeout(hide, 3000)) : (dailyShown = false));
    dailyTimer = setTimeout(hide, 12000);
  }
  async function openDaily() {
    if (dailyShown) {
      clearTimeout(dailyTimer);
      dailyShown = false;
      return;
    }
    if (!daily.item) {
      await checkDaily(libraries);
      if (!daily.item) {
        notice = "今天的句子还没有：需要先为知识库建立索引。";
        return;
      }
    }
    showDaily();
  }
  async function anotherDaily() {
    try {
      await dailyAgain();
      showDaily();
    } catch (e) {
      error = String(e);
    }
  }
  function openDailySource() {
    const d = daily.item;
    workspace
      .openVolume({ lib: d.lib, bookId: d.book_id, path: d.path, lb: d.lb || null, query: d.sentence, phrase: true })
      .catch((e) => (error = String(e)));
    dailyShown = false;
  }
  $effect(() => {
    // first check a little after start (indexes need to be open), then every 30 minutes
    const first = setTimeout(() => checkDaily(libraries), 15000);
    const every = setInterval(() => checkDaily(libraries), 30 * 60 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  });
  onMount(() => {
    checkUpdates();
    refresh().then(() => st?.libraries.length && api.prepareTitles().catch(() => {}));
    refreshMcpStatus();
    const offKeys = installShortcuts((type) => shortcut(type, readerSelection()));
    api.appInfo().then((i) => (appInfo = i)).catch(() => {});
    const focusMain = async () => {
      const w = getCurrentWindow();
      await w.unminimize().catch(() => {});
      await w.setFocus().catch(() => {});
    };
    // what the windows of their own ask of this one
    const offs = [
      listen("popout-action", (e) => {
        focusMain();
        readerAction(e.payload);
      }),
      listen("popout-shortcut", (e) => {
        focusMain();
        shortcut(e.payload.type, e.payload.text || "");
      }),
      listen("panel-open", (e) => {
        focusMain();
        if (e.payload.kind === "recent") openRecentItem(e.payload.item);
        else openMark(e.payload.item);
      }),
      listen("panel-ready", () => sendDoc()),
      listen("ai-note-created", async (e) => {
        await refresh();
        if (e.payload?.lib && e.payload?.id) {
          await libPanel?.showNew(e.payload, false);
        }
        api.prepareTitles().catch(() => {});
      }),
      listen("library-mutated", async (e) => {
        const lib = e.payload?.lib;
        const path = e.payload?.path;
        await refresh();
        if (lib) libPanel?.reloadNode?.(lib, "");
        if (lib && path) workspace.refreshDocument(lib, path);
      }),
    ];
    const un0a = api.onTitlesProgress((p) => (titlesProgress[p.lib] = p));
    const un0b = api.onTitlesDone((d) => delete titlesProgress[d.lib]);
    const un1 = api.onIndexProgress((p) => (progress[p.lib] = p));
    const un2 = api.onIndexDone((d) => {
      delete progress[d.lib];
      const l = libraries.find((x) => x.id === d.lib);
      if (d.ok) notice = `${l?.title ?? d.lib}：${d.message}`;
      else error = `${l?.title ?? d.lib}：${d.message}`;
      refresh();
    });
    const flush = () => workspace.flush();
    window.addEventListener("beforeunload", flush);
    return () => {
      un0a.then((f) => f());
      un0b.then((f) => f());
      un1.then((f) => f());
      un2.then((f) => f());
      window.removeEventListener("beforeunload", flush);
      offKeys();
      offs.forEach((p) => p.then((f) => f()));
    };
  });

  $effect(() => {
    workspace.setStartState({
      libraryCount: libraries.length,
      aiConfigured,
      mcpRunning: mcpState.running,
    });
  });

  $effect(() => {
    if (wsEl && st && !wsReady) {
      wsReady = true;
      workspace.init(wsEl, {
        libraries: st.libraries,
        onlocate: locate,
        onaction: readerAction,
        onai: handleAiRequest,
        onaidock: (payload) => {
          setAiOpen(true);
          if (payload?.sessionId) {
            aiIncoming = { nonce: ++aiSeq, sessionId: payload.sessionId };
          }
        },
        oncopypath: copyTabPath,
        ontreepath: (d) => libPanel?.treePath(d),
        onnodepath: (v) => libPanel?.nodePath(v.lib, { bookId: v.book_id ?? null, path: v.path }) || "",
        startActions: {
          onbind: (kind = "") => (binding = { path: "", kind }),
          onsearch: () => openSearch(),
          onquick: () => openQuickSearch(),
          onai: () => {
            if (!settings.aiOn) updateSettings({ aiOn: true });
            setAiOpen(true);
          },
          onsettings: (tab) => openSettings(tab),
          onnewnote: () => libPanel?.newNote(),
          onnewmindmap: () => libPanel?.newMindMap(),
          onnewwhiteboard: () => libPanel?.newWhiteboard(),
          ondiary: () => libPanel?.openDiary(),
        },
      }).catch((e) => (error = String(e)));
    }
  });

  /** Update library status and auto-start indexing for any newly created library (e.g. default diary/notes). */
  function applyLibrariesStatus(nextStatus) {
    const before = new Set(libraries.map((l) => l.id));
    st = nextStatus;
    const added = (nextStatus?.libraries ?? []).filter((l) => !before.has(l.id));
    for (const l of added) {
      if (!l.index_ready && !l.building && !l.error) startIndex(l.id);
    }
  }

  async function refresh() {
    try {
      st = await api.status();
    } catch (e) {
      error = String(e);
    }
  }

  async function openNode(lib, node) {
    try {
      await workspace.openVolume({ lib: lib.id, bookId: node.book_id, path: node.path ?? node.start });
    } catch (e) {
      error = String(e);
    }
  }

  async function openQuick(item) {
    try {
      if (item.kind === "folder") {
        // a folder is not read: show it in the tree
        if (!(await libPanel?.reveal({ lib: item.lib, path: item.path }))) error = "目录树里找不到这个文件夹";
        return;
      }
      // a citation's quote is highlighted as a phrase
      await workspace.openVolume({
        lib: item.lib,
        bookId: item.book_id,
        path: item.path,
        lb: item.lb,
        query: item.quote ?? null,
        phrase: !!item.quote,
      });
    } catch (e) {
      error = String(e);
    }
  }

  function bound(newStatus, meta = null) {
    const before = new Set(libraries.map((l) => l.id));
    st = newStatus;
    binding = null;
    const added = newStatus.libraries.filter((l) => !before.has(l.id));
    const toIndex = added.filter((l) => !l.index_ready && !l.building && !l.error);
    for (const l of toIndex) startIndex(l.id);
    if (meta?.imported) {
      notice = toIndex.length
        ? `已导入「${meta.title}」：${meta.count} ${meta.unit}，正在后台建立索引…`
        : `已导入「${meta.title}」：${meta.count} ${meta.unit}。`;
    } else {
      notice = added.length
        ? toIndex.length
          ? `已添加：${added.map((l) => l.title).join("、")}，正在后台建立索引…`
          : `已添加：${added.map((l) => l.title).join("、")}。`
        : "目录树已更新。";
    }
    api.prepareTitles().catch(() => {});
  }

  let unbindRemoteDlg = $state(null);
  let gitStatus = $state({}); // lib id -> { dirty, has_update, error }
  let gitUpdating = $state({}); // lib id -> true
  let gitNotified = new Set();

  async function checkGitStatus(fetch = false, notifyUpdates = false) {
    if (!libraries.some((l) => l.remote)) return;
    try {
      const list = await api.gitStatusAll(fetch);
      const newlyAvailable = [];
      for (const item of list || []) {
        gitStatus[item.lib] = item;
        if (notifyUpdates && item.has_update && !item.dirty && !gitNotified.has(item.lib)) {
          gitNotified.add(item.lib);
          const l = libraries.find((x) => x.id === item.lib);
          if (l) newlyAvailable.push(l.title);
        }
        if (!item.has_update) gitNotified.delete(item.lib);
      }
      if (newlyAvailable.length) {
        notice = `远程知识库「${newlyAvailable.join("、")}」有可用更新，可在目录树点击「有更新」或右键更新。`;
      }
    } catch {}
  }

  $effect(() => {
    const hasRemotes = libraries.some((l) => l.remote);
    if (!hasRemotes) return;
    const first = setTimeout(() => checkGitStatus(true, true), 20000);
    const every = setInterval(() => checkGitStatus(true, true), 30 * 60 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  });

  async function updateRemoteLibrary(l) {
    if (gitUpdating[l.id]) return;
    gitUpdating[l.id] = true;
    try {
      const r = await api.gitPullLibrary(l.id);
      gitStatus[l.id] = { lib: l.id, dirty: false, has_update: false, error: null };
      gitNotified.delete(l.id);
      if (r.updated) {
        await libPanel?.reloadNode(l.id, null);
        startIndex(l.id);
        notice = `已更新「${r.title}」，正在后台更新索引…`;
        api.prepareTitles().catch(() => {});
      } else {
        notice = `「${r.title}」已是最新版本。`;
      }
    } catch (e) {
      error = String(e);
      checkGitStatus(false, false);
    } finally {
      delete gitUpdating[l.id];
    }
  }

  async function confirmUnbindRemote(l, deleteLocal) {
    await workspace.closeLibrary(l.id);
    st = await api.unbindLibrary(l.id, deleteLocal);
    delete gitStatus[l.id];
    gitNotified.delete(l.id);
    unbindRemoteDlg = null;
    notice = deleteLocal ? `已解除绑定并删除本地数据：${l.title}` : `已解除绑定：${l.title}`;
  }

  async function unbind(l) {
    if (l.remote) {
      unbindRemoteDlg = l;
      return;
    }
    const own = l.kind === "notes";
    const ok = await ask(
      own
        ? `删除知识库「${l.title}」？\n里面的全部笔记会被永久删除，不能恢复。`
        : `解除绑定「${l.title}」？\n它的索引会被删除；数据包本身不受影响，以后可以重新绑定。`,
      {
        title: own ? "删除知识库" : "解除绑定",
        kind: "warning",
        okLabel: own ? "永久删除" : "解除绑定",
        cancelLabel: "取消",
      },
    );
    if (!ok) return;
    try {
      await workspace.closeLibrary(l.id);
      st = await api.unbindLibrary(l.id);
      notice = `${own ? "已删除" : "已解除绑定"}：${l.title}`;
    } catch (e) {
      error = String(e);
    }
  }

  async function startIndex(id) {
    try {
      progress[id] = { lib: id, done: 0, total: 1 };
      await api.buildIndex(id);
    } catch (e) {
      delete progress[id];
      error = String(e);
    }
  }
</script>

<div class="app">

  <div class="body">
    <aside class="rail" aria-label="主功能栏">
      <div class="rail-top">
        <button class="rail-btn" class:on={sideOpen} onclick={() => setSideOpen(!sideOpen)}
          title={sideOpen ? "收起知识库目录" : "展开知识库目录"} aria-label="知识库目录" aria-pressed={sideOpen}>
          <Icon name="library" size={18} /><span>目录</span>
        </button>
        <button class="rail-btn" onclick={openSearch}
          title={formatShortcut("search", settings.shortcuts) ? `全文搜索（${formatShortcut("search", settings.shortcuts)}）` : "全文搜索"} aria-label="全文搜索">
          <Icon name="search" size={18} /><span>搜索</span>
        </button>
        <button class="rail-btn" onclick={openQuickSearch}
          title={`标题快搜（双击 Shift${formatShortcut("quick", settings.shortcuts) ? ` 或 ${formatShortcut("quick", settings.shortcuts)}` : ""}）`} aria-label="标题快搜">
          <Icon name="plane" size={18} /><span>快搜</span>
        </button>
        <button class="rail-btn" onclick={() => openDictWindow("")}
          title={formatShortcut("dict", settings.shortcuts) ? `查词典（${formatShortcut("dict", settings.shortcuts)}）` : "查词典"} aria-label="查词典">
          <Icon name="dict" size={18} /><span>词典</span>
        </button>
        <button class="rail-btn" onclick={openMarks}
          title={formatShortcut("marks", settings.shortcuts) ? `收藏与标签（${formatShortcut("marks", settings.shortcuts)}）` : "收藏与标签"} aria-label="收藏与标签">
          <Icon name="bookmark" size={18} /><span>收藏</span>
        </button>
        <button class="rail-btn" onclick={openRecent}
          title={formatShortcut("recent", settings.shortcuts) ? `最近访问（${formatShortcut("recent", settings.shortcuts)}）` : "最近访问"} aria-label="最近访问">
          <Icon name="clock" size={18} /><span>最近</span>
        </button>
        {#if settings.aiOn}
          <button class="rail-btn ai-rail" class:on={aiOpen} onclick={() => setAiOpen(!aiOpen)}
            title={formatShortcut("ai", settings.shortcuts) ? `打开 / 收起右侧 AI 助手栏（${formatShortcut("ai", settings.shortcuts)}）` : "打开 / 收起右侧 AI 助手栏"}
            aria-label="AI 助手" aria-pressed={aiOpen}>
            <Icon name="sparkles" size={18} /><span>AI</span>
            {#if !aiConfigured}<span class="rail-dot warn" title="待配置模型"></span>{/if}
          </button>
        {/if}
      </div>
      {#if !sideOpen}
        <div class="rail-mid" aria-label="视图切换">
          <button class="rail-nav-btn" class:off={!workspace.ws.canBack} aria-disabled={!workspace.ws.canBack}
            onclick={() => workspace.ws.canBack && workspace.navigateHistory(-1)} oncontextmenu={openViewHistory}
            title={`上一个视图（${formatShortcut("nav-back", settings.shortcuts) ? `${formatShortcut("nav-back", settings.shortcuts)}；` : ""}右键：视图历史）`} aria-label="上一个视图">
            <Icon name="arrow-left" size={18} />
          </button>
          <button class="rail-nav-btn" class:off={!workspace.ws.canForward} aria-disabled={!workspace.ws.canForward}
            onclick={() => workspace.ws.canForward && workspace.navigateHistory(1)} oncontextmenu={openViewHistory}
            title={`下一个视图（${formatShortcut("nav-forward", settings.shortcuts) ? `${formatShortcut("nav-forward", settings.shortcuts)}；` : ""}右键：视图历史）`} aria-label="下一个视图">
            <Icon name="arrow-right" size={18} />
          </button>
        </div>
      {/if}
      <div class="rail-foot">
        {#if activeTasks.length}
          <button class="rail-btn task-rail" class:on={showTasks}
            onclick={() => { showTasks = !showTasks; if (!showTasks) tasksDismissed = true; }}
            title={`进行中的后台任务（${activeTasks.length} 项）— 点击查看详情`} aria-label="后台任务进度">
            <svg class="ring" class:spin={taskOverallPct == null} viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
              <circle class="ring-bg" cx="10" cy="10" r={RING_R} />
              <circle class="ring-fg" cx="10" cy="10" r={RING_R}
                stroke-dasharray={RING_C}
                stroke-dashoffset={taskOverallPct == null ? RING_C * 0.72 : RING_C * (1 - taskOverallPct / 100)} />
            </svg>
            <span>{taskOverallPct == null ? "进度" : `${taskOverallPct}%`}</span>
          </button>
        {/if}
        <button class="rail-btn" class:mcp-on={mcpState.running} onclick={() => openSettings("mcp")}
          title={mcpState.running ? `本地 Agent (MCP) 运行中：${mcpState.url}（点击查看接入配置）` : "本地 Agent (MCP) 未开启（点击配置外部 AI 客户端接入）"}
          aria-label="本地 Agent (MCP)">
          <Icon name="sliders" size={17} /><span>MCP</span>
          <span class="rail-dot" class:ok={mcpState.running}></span>
        </button>
        {#if settings.dailyOn}
          <button class="rail-btn" class:on={dailyShown} onclick={openDaily} title="每日一句" aria-label="每日一句">
            <Icon name="speaker" size={17} /><span>一句</span>
            {#if daily.item && !daily.seen}<span class="badge">1</span>{/if}
          </button>
        {/if}
        <button class="rail-btn" class:on={showLog} onclick={toggleLog} title="本次运行期间的消息" aria-label="消息">
          <Icon name="message" size={17} /><span>消息</span>
          {#if unreadLog}<span class="badge">{unreadLog}</span>{/if}
        </button>
        <button class="rail-btn" onclick={() => openSettings("appearance")}
          title={formatShortcut("settings", settings.shortcuts) ? `设置（${formatShortcut("settings", settings.shortcuts)}）` : "设置"} aria-label="设置">
          <Icon name="gear" size={17} /><span>设置</span>
        </button>
        <button class="rail-btn" onclick={() => (aboutBox = true)} title="关于" aria-label="关于">
          <Icon name="info" size={17} /><span>关于</span>
        </button>
      </div>
    </aside>

    <nav class="side" class:collapsed={!sideOpen} aria-label="知识库目录" style="width: {sideWidth}px">
      <div class="side-head">
        <span class="side-title">知识库</span>
        <span class="side-head-actions">
          <button class="add-btn" onclick={(e) => { const r = e.currentTarget.getBoundingClientRect(); libPanel?.addMenu(r.left, r.bottom + 2); }} title="添加知识库、笔记、脑图或白板" aria-label="添加">
            <Icon name="plus" size={14} /><span>添加</span>
          </button>
          <span class="side-nav">
            <button class="icon" class:off={!workspace.ws.canBack} aria-disabled={!workspace.ws.canBack}
              onclick={() => workspace.ws.canBack && workspace.navigateHistory(-1)} oncontextmenu={openViewHistory}
              title={`上一个视图（${formatShortcut("nav-back", settings.shortcuts) ? `${formatShortcut("nav-back", settings.shortcuts)}；` : ""}右键：视图历史）`} aria-label="上一个视图">
              <Icon name="arrow-left" size={16} />
            </button>
            <button class="icon" class:off={!workspace.ws.canForward} aria-disabled={!workspace.ws.canForward}
              onclick={() => workspace.ws.canForward && workspace.navigateHistory(1)} oncontextmenu={openViewHistory}
              title={`下一个视图（${formatShortcut("nav-forward", settings.shortcuts) ? `${formatShortcut("nav-forward", settings.shortcuts)}；` : ""}右键：视图历史）`} aria-label="下一个视图">
              <Icon name="arrow-right" size={16} />
            </button>
          </span>
        </span>
      </div>
      <div class="tree">
        {#if st}
          <LibraryPanel
            bind:this={libPanel}
            {libraries}
            {progress}
            titles={titlesProgress}
            {gitStatus}
            {gitUpdating}
            onopen={openNode}
            onbind={(kind = "") => (binding = { path: "", kind: typeof kind === "string" ? kind : "" })}
            onrebind={(l) => (binding = { path: l.source, kind: "" })}
            onunbind={unbind}
            onindex={startIndex}
            onupdate={updateRemoteLibrary}
            ongitstatus={(fetch) => checkGitStatus(fetch, false)}
            onsearchfrom={searchFrom}
            ontag={tagFromTree}
            onprops={propsFromTree}
            oncopy={copyPath}
            onexport={(lib, sel, label, name, nodePath = "") => (exportDlg = { lib, sel, label, name, nodePath })}
            onimport={importLibrary}
            onlibs={async () => applyLibrariesStatus(await api.status())}
            onnotice={(msg) => (notice = msg)}
            onerror={(e) => (error = e)} />
        {:else}
          <p class="muted">载入中…</p>
        {/if}
      </div>
    </nav>
    {#if sideOpen}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="splitter" class:dragging role="separator" aria-orientation="vertical" aria-label="调整目录宽度"
        aria-valuenow={sideWidth} tabindex="0" onpointerdown={startDrag} onkeydown={splitterKey}
        ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>
    {/if}
    <main class="content" bind:this={wsEl}></main>
    {#if settings.aiOn && aiOpen}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="splitter ai-splitter" class:dragging={aiDragging} role="separator" aria-orientation="vertical" aria-label="调整 AI 助手宽度"
        aria-valuenow={aiWidth} tabindex="0" onpointerdown={startAiDrag} onkeydown={aiSplitterKey}
        ondblclick={() => (aiWidth = 380)} title="拖动调整宽度，双击还原"></div>
      <div class="ai-dock" style="width: {aiWidth}px">
        <AiPanel
          scopeKind="unified"
          doc={workspace.ws.activeDoc}
          searchContext={workspace.ws.activeSearch}
          incoming={aiIncoming}
          onEditDoc={workspace.ws.activeDoc?.writable ? workspace.ws.activeDoc.editDoc : null}
          onOpenDoc={(req) => workspace.openVolume(req).catch((e) => (error = String(e)))}
          onLibraryChanged={(lib, path) => {
            refresh();
            if (lib) libPanel?.reloadNode?.(lib, "");
            if (lib && path) workspace.refreshDocument(lib, path);
          }}
          onSessionChange={({ sessionId }) => (aiSessionId = sessionId)}
          onPopout={() => {
            setAiOpen(false);
            workspace.openAiWindow({ sessionId: aiSessionId });
          }}
          onOpenSettings={(tab) => openSettings(tab)}
          onClose={() => setAiOpen(false)} />
      </div>
    {/if}
  </div>
</div>

{#if (dailyShown && daily.item) || (showTasks && activeTasks.length) || showLog}
  <div class="dock-popups">
    {#if dailyShown && daily.item}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="daily" role="status" onmouseenter={() => (dailyHover = true)} onmouseleave={() => (dailyHover = false)}>
        <div class="log-head">
          <strong>每日一句</strong>
          <span class="grow"></span>
          <button class="link" onclick={openDailySource}>打开原文</button>
          <button class="link" onclick={anotherDaily}>换一句</button>
          <button class="link" onclick={() => (dailyShown = false)}>关闭</button>
        </div>
        <p class="sentence">{daily.item.sentence}</p>
        <div class="src muted">
          《{daily.item.title}》{daily.item.subtitle}{daily.item.lb ? ` · ${daily.item.lb}` : ""}
        </div>
      </div>
    {/if}

    {#if showTasks && activeTasks.length}
      <div class="task-pop" role="status" aria-label="后台任务进度">
        <div class="log-head">
          <strong>后台任务（{activeTasks.length}）</strong>
          <span class="grow"></span>
          <button class="link" onclick={() => { showTasks = false; tasksDismissed = true; }}>收起</button>
        </div>
        <div class="task-list">
          {#each activeTasks as t (t.id)}
            <div class="task-item">
              <div class="task-row">
                <span class="task-title">{t.title}</span>
                <span class="muted task-meta">{t.pct != null ? `${t.pct}%（${t.detail}）` : t.detail}</span>
              </div>
              {#if t.pct != null}
                <progress value={t.pct} max="100"></progress>
              {:else}
                <progress></progress>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if showLog}
      <div class="log" role="dialog" aria-label="消息列表">
        <div class="log-head">
          <strong>消息</strong>
          <span class="grow"></span>
          <button class="link" onclick={() => { log = []; unreadLog = 0; }}>清空</button>
          <button class="link" onclick={() => (showLog = false)}>关闭</button>
        </div>
        {#each log as m (m.id)}
          <div class="log-item" class:error={m.kind === "error"}><time>{m.time}</time> {m.text}</div>
        {:else}
          <p class="muted">还没有消息。</p>
        {/each}
      </div>
    {/if}
  </div>
{/if}

{#if toast}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="toast" class:error={toast.kind === "error"} role={toast.kind === "error" ? "alert" : "status"}
    onmouseenter={() => (toastHover = true)} onmouseleave={() => (toastHover = false)}>
    <span>{toast.text}</span>
    <button class="link" onclick={() => ((toast = null), (toastHover = false))} aria-label="关闭">×</button>
  </div>
{/if}

{#if exportDlg}
  <ExportDialog lib={exportDlg.lib} sel={exportDlg.sel} label={exportDlg.label} name={exportDlg.name} nodePath={exportDlg.nodePath}
    ondone={(r) => (notice = `已导出 ${r.docs} 项：${r.path}`)} onclose={() => (exportDlg = null)} />
{/if}

{#if importing}
  <div class="import-mask" role="dialog" aria-modal="true" aria-label="正在导入">
    <div class="dialog dialog-shell import-busy">
      <div class="dialog-content">
        <p style="margin: 0 0 8px; font-weight: 500;">正在导入数据包…</p>
        <progress style="width: 100%;"></progress>
        <p class="muted" style="margin: 8px 0 0; font-size: 13px;">正在解压并写入笔记，请勿关闭窗口。</p>
      </div>
    </div>
  </div>
{/if}

{#if clipDlg}
  <ClipDialog text={clipDlg.text} vol={clipDlg.vol} onok={(req) => doClip(req, clipDlg)} onclose={() => (clipDlg = null)} />
{/if}

{#if propsDlg}
  <PropsDialog {...propsDlg} onclose={() => (propsDlg = null)}
    onsaved={(r) => (notice = r.updating ? "属性已保存，正在更新索引…" : "属性已保存。建立索引后才能用在搜索过滤里。")} />
{/if}

{#if tagDlg}
  <TagDialog {...tagDlg} onclose={() => (tagDlg = null)} />
{/if}

{#if marksBox}
  <MarksDialog doc={workspace.ws.current} onopen={openMark} onclose={() => (marksBox = false)} onpopout={() => popoutPanel("marks")} />
{/if}

{#if pinyinDlg}
  <PinyinDialog text={pinyinDlg} onclose={() => (pinyinDlg = "")} />
{/if}

<UpdateDialog />

{#if aboutBox}
  <AboutDialog info={appInfo} oncheckupdate={() => { aboutBox = false; checkUpdates(true); }} onclose={() => (aboutBox = false)} />
{/if}

{#if settingsBox}
  <SettingsDialog
    dataDir={appInfo?.data_dir ?? ""}
    initialTab={settingsTab}
    onmcpchange={(next) => (mcpState = { ...mcpState, ...next })}
    onclose={() => { settingsBox = false; refreshMcpStatus(); }} />
{/if}

{#if recentBox}
  <RecentDialog onopen={openRecentItem} onclose={() => (recentBox = false)} onpopout={() => popoutPanel("recent")} />
{/if}

{#if viewHistoryBox}
  <ViewHistoryDialog onclose={() => (viewHistoryBox = false)} />
{/if}

{#if quick}
  <QuickSearch {libraries} scope={quickScope} initialQuery={quickQuery} onopen={openQuick} onclose={() => (quick = false)} />
{/if}

{#if binding}
  <BindDialog initialPath={binding.path} initialKind={binding.kind} existingTitles={libraries.map((l) => l.title)} onclose={() => (binding = null)} onbound={bound} />
{/if}

{#if unbindRemoteDlg}
  <UnbindRemoteDialog lib={unbindRemoteDlg} onconfirm={confirmUnbindRemote} onclose={() => (unbindRemoteDlg = null)} />
{/if}

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  .badge {
    position: absolute; top: -2px; right: -4px; min-width: 14px; padding: 0 3px; border-radius: 7px;
    background: var(--accent); color: var(--paper); font-size: 10px; line-height: 14px; text-align: center;
  }
  .dock-popups {
    position: fixed; left: 64px; bottom: 16px; z-index: 995;
    display: flex; flex-direction: column; gap: 10px; align-items: flex-start;
    max-height: calc(100vh - 32px); pointer-events: none;
  }
  .dock-popups > * { pointer-events: auto; }
  .daily {
    width: min(460px, calc(100vw - 80px));
    padding: 10px 14px; border-radius: 8px; background: var(--paper); border: 1px solid var(--accent);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
  }
  .daily .sentence { margin: 2px 0 6px; font-size: 17px; line-height: 1.75; }
  .daily .src { font-size: 12.5px; }
  .toast {
    position: fixed; right: 16px; bottom: 16px; z-index: 1000; max-width: min(520px, calc(100vw - 32px));
    display: flex; gap: 10px; align-items: flex-start; padding: 10px 14px; border-radius: 8px;
    background: var(--paper); border: 1px solid var(--line); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .toast.error { background: var(--hl-bg); color: var(--hl); border-color: var(--hl); }
  .log {
    width: min(460px, calc(100vw - 80px)); max-height: 45vh;
    overflow: auto; padding: 10px 14px; border-radius: 8px; background: var(--paper);
    border: 1px solid var(--line); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
  }
  .log-head { display: flex; gap: 10px; align-items: center; margin-bottom: 6px; }
  .grow { flex: 1; }
  .log-item { padding: 4px 0; border-top: 1px solid var(--line); font-size: 13px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .log-item.error { color: var(--hl); }
  .log-item time { color: var(--muted); margin-right: 6px; }
  .body { flex: 1; display: flex; min-height: 0; }
  .rail {
    flex: none; width: 54px; display: flex; flex-direction: column; justify-content: space-between;
    background: color-mix(in srgb, var(--panel) 72%, var(--bg));
    border-right: 1px solid var(--line); padding: 6px 4px; gap: 6px; min-height: 0;
  }
  .rail-top, .rail-mid, .rail-foot {
    display: flex; flex-direction: column; align-items: center; gap: 8px;
  }
  .rail-top { overflow-y: auto; overflow-x: hidden; scrollbar-width: none; }
  .rail-mid { flex: none; margin: auto 0; gap: 4px; }
  .rail-foot { flex: none; padding-top: 8px; border-top: 1px solid var(--line); }
  .rail-top > .rail-btn + .rail-btn::before,
  .rail-foot > .rail-btn + .rail-btn::before {
    content: ""; position: absolute; top: -5px; left: 50%; transform: translateX(-50%);
    width: 8px; height: 2px; border-radius: 999px;
    background: color-mix(in srgb, var(--line) 75%, var(--muted));
    opacity: 0.25; pointer-events: none;
  }
  .rail-nav-btn {
    display: inline-flex; align-items: center; justify-content: center; width: 46px; height: 30px;
    border: none; border-radius: 7px; background: transparent; color: var(--accent); padding: 0;
    opacity: 0.55; transition: opacity 0.15s ease, background 0.15s ease;
  }
  .rail-nav-btn:hover { opacity: 1; background: var(--accent-soft); }
  .rail-nav-btn.off, .rail-nav-btn.off:hover { color: var(--muted); opacity: 0.22; cursor: default; background: transparent; }
  .rail-btn {
    position: relative; width: 46px; padding: 5px 2px 4px;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px;
    border: none; border-radius: 7px; background: transparent; color: var(--muted);
    font-size: 10.5px; line-height: 1.15; white-space: nowrap;
  }
  .rail-btn:hover { background: var(--accent-soft); color: var(--accent); }
  .rail-btn.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .rail-btn.ai-rail { color: var(--accent); }
  .rail-btn.ai-rail.on { background: var(--accent); color: var(--paper); }
  .rail-btn.mcp-on { color: var(--note); }
  .rail-btn.task-rail { color: var(--accent); }
  .ring { transform: rotate(-90deg); display: block; }
  .ring.spin { animation: rail-spin 1.1s linear infinite; }
  @keyframes rail-spin { from { transform: rotate(-90deg); } to { transform: rotate(270deg); } }
  .ring-bg { fill: none; stroke: var(--line); stroke-width: 2.2; }
  .ring-fg { fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; transition: stroke-dashoffset 0.2s ease; }
  .task-pop {
    width: min(460px, calc(100vw - 80px));
    padding: 10px 14px; border-radius: 8px; background: var(--paper);
    border: 1px solid var(--accent); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
  }
  .task-list { display: flex; flex-direction: column; gap: 8px; }
  .task-item { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; }
  .task-row { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .task-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; }
  .task-meta { flex: none; font-size: 11.5px; }
  .task-item progress { width: 100%; height: 6px; accent-color: var(--accent); }
  .rail-btn .badge { top: 1px; right: 2px; }
  .rail-dot {
    position: absolute; top: 5px; right: 7px; width: 6px; height: 6px; border-radius: 50%;
    background: var(--muted); opacity: 0.45;
  }
  .rail-dot.ok { background: var(--note); opacity: 1; }
  .rail-dot.warn { background: var(--accent); opacity: 0.85; }
  .rail-btn.ai-rail.on .rail-dot.warn { background: var(--paper); }
  .side {
    flex: none; display: flex; flex-direction: column; gap: 0; padding: 0;
    background: var(--panel); min-height: 0;
  }
  .side.collapsed { display: none; }
  .splitter {
    flex: none; width: 5px; cursor: col-resize; background: var(--panel);
    border-right: 1px solid var(--line); touch-action: none;
  }
  .splitter:hover, .splitter.dragging, .splitter:focus-visible { background: var(--accent-soft); outline: none; }
  .import-mask {
    position: fixed; inset: 0; z-index: 960; background: rgba(0, 0, 0, 0.35);
    display: flex; align-items: center; justify-content: center;
  }
  .import-busy { width: min(380px, calc(100vw - 32px)); }
  .body:has(.splitter.dragging) { user-select: none; cursor: col-resize; }
  .side-head {
    display: flex; align-items: center; justify-content: space-between; gap: 6px;
    padding: 6px 8px 6px 12px; border-bottom: 1px solid var(--line); font-size: 13px;
  }
  .side-title { font-weight: 600; color: var(--ink); white-space: nowrap; }
  .side-head-actions { display: flex; align-items: center; gap: 4px; }
  .side-nav { display: inline-flex; align-items: center; gap: 2px; }
  .icon {
    display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 24px;
    border: none; background: none; padding: 0; color: var(--accent); border-radius: 6px;
  }
  .icon:hover { background: var(--accent-soft); }
  .icon.off, .icon:disabled { color: var(--muted); opacity: 0.4; cursor: default; background: none; }
  .add-btn {
    display: inline-flex; align-items: center; gap: 3px; padding: 2px 8px;
    border: 1px solid var(--line); border-radius: 6px; background: var(--paper);
    color: var(--accent); font-size: 12px; line-height: 1.3; white-space: nowrap;
  }
  .add-btn:hover { border-color: var(--accent); background: var(--accent-soft); }
  .tree { flex: 1; overflow: auto; font-size: 15px; padding: 6px 8px; }
  .content { flex: 1; min-width: 0; min-height: 0; position: relative; }
  .ai-splitter { border-right: none; border-left: 1px solid var(--line); }
  .ai-dock { flex: none; height: 100%; min-height: 0; display: flex; flex-direction: column; }
  .ai-dock > :global(*) { flex: 1; min-height: 0; }
</style>
