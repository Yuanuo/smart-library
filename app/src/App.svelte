<script>
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api.js";
  import * as workspace from "./lib/workspace.svelte.js";
  import LibraryPanel from "./lib/LibraryPanel.svelte";
  import BindDialog from "./lib/BindDialog.svelte";
  import QuickSearch from "./lib/QuickSearch.svelte";
  import Icon from "./lib/Icon.svelte";
  import RecentDialog from "./lib/RecentDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import TagDialog from "./lib/TagDialog.svelte";
  import MarksDialog from "./lib/MarksDialog.svelte";
  import PropsDialog from "./lib/PropsDialog.svelte";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import UpdateDialog from "./lib/UpdateDialog.svelte";
  import { checkUpdates } from "./lib/updater.svelte.js";
  import { installShortcuts, readerSelection } from "./lib/shortcuts.js";
  import { localDate, localTime } from "./lib/diary.js";
  import { settings, update as updateSettings } from "./lib/settings.svelte.js";
  import { daily, check as checkDaily, again as dailyAgain, markSeen as dailySeen } from "./lib/daily.svelte.js";
  import { openPanelWindow } from "./lib/panels.js";
  import { fullPath } from "./lib/paths.js";
  import ClipDialog from "./lib/ClipDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import { open as openFile } from "@tauri-apps/plugin-dialog";
  import { clipMarkdown, clipNode } from "./lib/clip.js";
  import { getLast, setLast } from "./lib/clipTarget.js";
  import { emit, listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let st = $state(null);
  let error = $state("");
  let notice = $state("");

  // Messages appear as a toast over the window (the layout never moves) and
  // are kept in a list for the rest of the run. A toast goes away after a few
  // seconds, but stays while the mouse rests on it or nothing was touched
  // since it appeared (the user may be away).
  let log = $state([]); // { id, time, kind, text }, newest first
  let toast = $state(null);
  let showLog = $state(false);
  let lastActive = Date.now();
  let toastTimer = null;
  let toastSeq = 0;
  let toastHover = false;

  function push(kind, text) {
    const m = { id: ++toastSeq, time: new Date().toLocaleTimeString("zh-CN", { hour12: false }), kind, text };
    log = [m, ...log].slice(0, 200);
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
    else if (type === "recent") openRecent();
    else if (type === "marks") openMarks();
    else if (type === "settings") settingsBox = true;
    else if (type === "diary") libPanel?.openDiary();
    else if (type === "new") libPanel?.newNote();
    else if (type === "close") workspace.closeActive();
    else if (type === "next-tab") workspace.cycleTab(1);
    else if (type === "prev-tab") workspace.cycleTab(-1);
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
    else if (type === "props") {
      const l = libraries.find((x) => x.id === c.lib);
      if (l) propsDlg = { lib: l.id, libTitle: l.title, bookId: l.kind === "cbeta" ? c.bookId : null, path: l.kind === "cbeta" ? "" : c.path, title: c.title };
    }
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
    } else if (a.type === "error") {
      error = a.text;
    } else if (a.type === "quick") {
      quickScope = null;
      quickQuery = a.text;
      recentBox = false;
      quick = true;
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

  /** `target`: { lib, parent, key } to import below a place of an internal library; none: as a new library. */
  async function importLibrary(target = null) {
    try {
      const path = await openFile({
        multiple: false,
        title: "选择导出包",
        filters: [{ name: "知识库导出包（*.smartlib）", extensions: ["smartlib"] }, { name: "ZIP", extensions: ["zip"] }],
      });
      if (!path) return;
      notice = "正在导入…";
      const r = await api.importPackage(path, target?.lib ?? null, target?.parent ?? null);
      st = r.status;
      if (target) {
        await libPanel?.reloadNode(target.lib, target.key);
        notice = `已导入到「${r.title}」：${r.notes} 篇笔记。`;
      } else {
        notice = `已导入「${r.title}」：${r.notes} 篇笔记。右键知识库可以建立索引。`;
      }
      api.prepareTitles().catch(() => {});
    } catch (e) {
      error = String(e);
    }
  }

  // ---- clippings: selected text into a note
  /** Add the selected text to today's diary (made when missing), after the time. */
  async function sendToDiary(a) {
    const r = await api.diaryToday(localDate());
    st = r.status;
    const md = `**${localTime()}**

` + (await clipMarkdown({ text: a.text, vol: a.vol, lb: a.lb, comment: "" }));
    const t = r.note;
    if (!workspace.appendIfEditing(t.lib, t.id, md)) {
      await api.noteAppend(t.lib, t.id, md);
      workspace.refreshDocument(t.lib, t.id);
    }
    await libPanel?.showNew(t, false);
    notice = `已发送到今日记：${localDate()}`;
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
        tagNode(v.lib, v.book_id ?? null, v.path, docTitle(v));
      }
    } catch (e) {
      error = String(e);
    }
  }

  /** Tags of a whole book / article. */
  async function tagNode(lib, bookId, path, title) {
    const current = await api.nodeTagsGet(lib, bookId, path);
    tagDlg = {
      heading: "标签", subtitle: title, tags: current.join(", "),
      onok: async (tags) => {
        await api.nodeTagsSet(lib, bookId, path, title, tags);
        notice = tags.trim() ? "标签已保存。" : "已清除标签。";
        ping();
      },
    };
  }

  let propsDlg = $state(null); // { lib, libTitle, bookId, path, title }
  /** The properties dialog of a node: CBETA the whole book; the others the article / file / folder / note. */
  function propsFromTree(l, node, treeKey = "") {
    if (!node) {
      propsDlg = { lib: l.id, libTitle: l.title, bookId: null, path: "", title: l.title };
      return;
    }
    const bookId = l.kind === "cbeta" ? node.book_id : null;
    const isNavFolder = (l.kind === "cbeta" || l.kind === "html") && !node.book_id && !node.path;
    const path = isNavFolder ? `@nav${treeKey}` : l.kind === "cbeta" ? "" : (node.path ?? node.rel);
    if (path == null) return;
    propsDlg = { lib: l.id, libTitle: l.title, bookId, path, title: node.title };
  }

  function tagFromTree(l, node) {
    // CBETA: the whole book; the others: the article / file / folder / note
    if (l.kind === "cbeta") {
      if (!node.book_id) return;
      tagNode(l.id, node.book_id, "", node.title).catch((e) => (error = String(e)));
    } else {
      tagNode(l.id, null, node.path ?? node.rel, node.title).catch((e) => (error = String(e)));
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
  let recentBox = $state(false); // reading history popup
  function openRecent() {
    quick = false;
    recentBox = true;
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

  // window title: the document being read (with its author), its library,
  // then the program and its version; just the program when nothing is open
  let appInfo = $state(null);
  $effect(() => {
    const c = workspace.ws.current;
    const lib = c ? libraries.find((l) => l.id === c.lib)?.title : "";
    const prog = `Smart Library${appInfo ? " " + appInfo.version : ""}`;
    const title = c
      ? [`${c.title}${c.author ? " by " + c.author : ""}`, lib ? `『${lib}』` : "", prog].filter(Boolean).join(" - ")
      : prog;
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

  // width of the tree panel: drag the splitter, kept in local storage
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
    const ok = await libPanel?.reveal(doc);
    if (!ok) notice = "目录树里没有找到这个文档。";
  }
  let wsReady = false;

  const libraries = $derived(st?.libraries ?? []);
  const buildingText = $derived.by(() => {
    const ids = Object.keys(progress);
    if (!ids.length) return "";
    return ids
      .map((id) => {
        const p = progress[id];
        const t = libraries.find((l) => l.id === id)?.title ?? id;
        return `${t}：建索引 ${Math.floor((100 * p.done) / Math.max(1, p.total))}%`;
      })
      .join("　");
  });

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
    ];
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
      un1.then((f) => f());
      un2.then((f) => f());
      window.removeEventListener("beforeunload", flush);
      offKeys();
      offs.forEach((p) => p.then((f) => f()));
    };
  });

  $effect(() => {
    if (wsEl && st && !wsReady) {
      wsReady = true;
      workspace.init(wsEl, { libraries: st.libraries, onlocate: locate, onaction: readerAction, oncopypath: copyTabPath, ontreepath: (d) => libPanel?.treePath(d) }).catch((e) => (error = String(e)));
    }
  });

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

  function bound(newStatus) {
    const before = new Set(libraries.map((l) => l.id));
    st = newStatus;
    binding = null;
    const added = newStatus.libraries.filter((l) => !before.has(l.id));
    notice = added.length
      ? `已绑定：${added.map((l) => l.title).join("、")}。右键知识库可以建立索引。`
      : "目录树已更新。";
    api.prepareTitles().catch(() => {});
  }

  async function unbind(l) {
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
    <nav class="side" aria-label="知识库目录" style="width: {sideWidth}px">
      <div class="side-head">
        <span class="muted">知识库</span>
        <span class="tools">
          <button class="icon" onclick={(e) => { const r = e.currentTarget.getBoundingClientRect(); libPanel?.addMenu(r.left, r.bottom + 2); }} title="添加" aria-label="添加"><Icon name="plus" size={18} /></button>
          <button class="icon" onclick={openQuickSearch} title="标题快搜（双击 Shift 或 Ctrl+G）" aria-label="标题快搜"><Icon name="plane" size={18} /></button>
          <button class="icon" onclick={openSearch} title="全文搜索（Ctrl+H）" aria-label="全文搜索"><Icon name="search" size={18} /></button>
          <button class="icon" onclick={openMarks} title="收藏与标签（Ctrl+B）" aria-label="收藏与标签"><Icon name="bookmark" size={18} /></button>
          <button class="icon" onclick={openRecent} title="最近打开（Ctrl+E）" aria-label="最近打开"><Icon name="clock" size={18} /></button>
        </span>
      </div>
      <div class="tree">
        {#if st}
          <LibraryPanel
            bind:this={libPanel}
            {libraries}
            {progress}
            onopen={openNode}
            onbind={() => (binding = { path: "" })}
            onrebind={(l) => (binding = { path: l.source })}
            onunbind={unbind}
            onindex={startIndex}
            onsearchfrom={searchFrom}
            ontag={tagFromTree}
            onprops={propsFromTree}
            oncopy={copyPath}
            onexport={(lib, sel, label, name) => (exportDlg = { lib, sel, label, name })}
            onimport={importLibrary}
            onlibs={async () => (st = await api.status())}
            onerror={(e) => (error = e)} />
        {:else}
          <p class="muted">载入中…</p>
        {/if}
      </div>
      <div class="side-foot">
        <span class="muted building">{buildingText}</span>
        <span class="tools">
          {#if settings.dailyOn}
            <button class="icon" onclick={openDaily} title="每日一句" aria-label="每日一句">
              <Icon name="speaker" size={18} />{#if daily.item && !daily.seen}<span class="badge">1</span>{/if}
            </button>
          {/if}
          <button class="icon" onclick={() => (settingsBox = true)} title="设置（Ctrl+,）" aria-label="设置"><Icon name="gear" size={18} /></button>
          <button class="icon" onclick={() => (showLog = !showLog)} title="本次运行期间的消息" aria-label="消息">
            <Icon name="message" size={18} />{#if log.length}<span class="badge">{log.length}</span>{/if}
          </button>
          <button class="icon" onclick={() => (aboutBox = true)} title="关于" aria-label="关于"><Icon name="info" size={18} /></button>
        </span>
      </div>
    </nav>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="splitter" class:dragging role="separator" aria-orientation="vertical" aria-label="调整目录宽度"
      aria-valuenow={sideWidth} tabindex="0" onpointerdown={startDrag} onkeydown={splitterKey}
      ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>
    <main class="content" bind:this={wsEl}></main>
  </div>
</div>

{#if dailyShown && daily.item}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="daily" role="status" onmouseenter={() => (dailyHover = true)} onmouseleave={() => (dailyHover = false)}>
    <p class="sentence">{daily.item.sentence}</p>
    <div class="src muted">
      《{daily.item.title}》{daily.item.subtitle}{daily.item.lb ? ` · ${daily.item.lb}` : ""}
      <button class="link" onclick={openDailySource}>打开原文</button>
      <button class="link" onclick={anotherDaily}>换一句</button>
      <button class="link" onclick={() => (dailyShown = false)} aria-label="关闭">×</button>
    </div>
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

{#if showLog}
  <div class="log" role="dialog" aria-label="消息列表">
    <div class="log-head">
      <strong>消息</strong>
      <span class="grow"></span>
      <button class="link" onclick={() => (log = [])}>清空</button>
      <button class="link" onclick={() => (showLog = false)}>关闭</button>
    </div>
    {#each log as m (m.id)}
      <div class="log-item" class:error={m.kind === "error"}><time>{m.time}</time> {m.text}</div>
    {:else}
      <p class="muted">还没有消息。</p>
    {/each}
  </div>
{/if}

{#if exportDlg}
  <ExportDialog lib={exportDlg.lib} sel={exportDlg.sel} label={exportDlg.label} name={exportDlg.name}
    ondone={(r) => (notice = `已导出 ${r.docs} 篇：${r.path}`)} onclose={() => (exportDlg = null)} />
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

<UpdateDialog />

{#if aboutBox}
  <AboutDialog info={appInfo} oncheckupdate={() => { aboutBox = false; checkUpdates(true); }} onclose={() => (aboutBox = false)} />
{/if}

{#if settingsBox}
  <SettingsDialog onclose={() => (settingsBox = false)} />
{/if}

{#if recentBox}
  <RecentDialog onopen={openRecentItem} onclose={() => (recentBox = false)} onpopout={() => popoutPanel("recent")} />
{/if}

{#if quick}
  <QuickSearch {libraries} scope={quickScope} initialQuery={quickQuery} onopen={openQuick} onclose={() => (quick = false)} />
{/if}

{#if binding}
  <BindDialog initialPath={binding.path} onclose={() => (binding = null)} onbound={bound} />
{/if}

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  .tools { display: flex; gap: 2px; }
  .icon {
    display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 26px;
    border: none; background: none; padding: 0; color: var(--accent); border-radius: 6px; font-size: 16px;
  }
  .icon:hover { background: var(--accent-soft); }
  .icon { position: relative; }
  .badge {
    position: absolute; top: -2px; right: -4px; min-width: 14px; padding: 0 3px; border-radius: 7px;
    background: var(--accent); color: var(--paper); font-size: 10px; line-height: 14px; text-align: center;
  }
  .side-foot {
    display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 4px 0;
    border-top: 1px solid var(--line); font-size: 12px; min-height: 32px;
  }
  .building { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .daily {
    position: fixed; left: 12px; bottom: 44px; z-index: 995; width: min(460px, calc(100vw - 32px));
    padding: 12px 16px; border-radius: 8px; background: var(--paper); border: 1px solid var(--accent);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
  }
  .daily .sentence { margin: 0 0 6px; font-size: 18px; line-height: 1.8; }
  .daily .src { font-size: 13px; display: flex; gap: 10px; align-items: baseline; flex-wrap: wrap; }
  .toast {
    position: fixed; right: 16px; bottom: 16px; z-index: 1000; max-width: min(520px, calc(100vw - 32px));
    display: flex; gap: 10px; align-items: flex-start; padding: 10px 14px; border-radius: 8px;
    background: var(--paper); border: 1px solid var(--line); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .toast.error { background: var(--hl-bg); color: var(--hl); border-color: var(--hl); }
  .log {
    position: fixed; left: 12px; bottom: 44px; z-index: 990; width: min(520px, calc(100vw - 32px)); max-height: 60vh;
    overflow: auto; padding: 10px 14px; border-radius: 8px; background: var(--paper);
    border: 1px solid var(--line); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
  }
  .log-head { display: flex; gap: 10px; align-items: center; margin-bottom: 6px; }
  .grow { flex: 1; }
  .log-item { padding: 4px 0; border-top: 1px solid var(--line); font-size: 13px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .log-item.error { color: var(--hl); }
  .log-item time { color: var(--muted); margin-right: 6px; }
  .body { flex: 1; display: flex; min-height: 0; }
  .side {
    flex: none; display: flex; flex-direction: column; gap: 6px; padding: 8px 8px;
    background: var(--panel); min-height: 0;
  }
  .splitter {
    flex: none; width: 5px; cursor: col-resize; background: var(--panel);
    border-right: 1px solid var(--line); touch-action: none;
  }
  .splitter:hover, .splitter.dragging, .splitter:focus-visible { background: var(--accent-soft); outline: none; }
  .body:has(.splitter.dragging) { user-select: none; cursor: col-resize; }
  .side-head {
    display: flex; justify-content: space-between; align-items: center; padding: 0 4px 6px; font-size: 13px;
    border-bottom: 1px solid var(--line);
  }
  .tree { flex: 1; overflow: auto; font-size: 15px; }
  .content { flex: 1; min-width: 0; min-height: 0; position: relative; }
</style>
