<script>
  import { onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api.js";
  import * as workspace from "./lib/workspace.svelte.js";
  import LibraryPanel from "./lib/LibraryPanel.svelte";
  import BindDialog from "./lib/BindDialog.svelte";
  import QuickSearch from "./lib/QuickSearch.svelte";
  import Icon from "./lib/Icon.svelte";
  import RecentDialog from "./lib/RecentDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
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

  let query = $state("");
  let phrase = $state(false);
  let searching = $state(false);
  let lastSearch = null; // query of the shown results, used for highlights

  let progress = $state({}); // lib id -> { done, total }
  let binding = $state(null); // null, or { path } for the bind dialog
  let quick = $state(false); // title quick search popup

  // Shift pressed twice (no other key in between, within 400 ms)
  let lastShift = 0;
  let shiftAlone = false;
  function shiftDown(e) {
    // Ctrl/Cmd+G: title quick search; Ctrl/Cmd+H: full-text search
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
      const k = e.key.toLowerCase();
      if (k === "," && !binding) {
        e.preventDefault();
        settingsBox = true;
        return;
      }
      if (k === "g" || k === "h" || k === "e") {
        e.preventDefault();
        shiftAlone = false;
        if (!binding) (k === "g" ? openQuickSearch : k === "h" ? openSearch : openRecent)();
        return;
      }
    }
    if (e.key === "Shift") {
      if (!e.repeat) shiftAlone = true;
    } else {
      shiftAlone = false;
      lastShift = 0;
    }
  }
  function shiftUp(e) {
    if (e.key !== "Shift" || !shiftAlone) return;
    const now = performance.now();
    if (now - lastShift < 400) {
      lastShift = 0;
      if (!binding) openQuickSearch();
    } else {
      lastShift = now;
    }
  }

  let quickQuery = $state(null); // text to start the quick search with

  /** Actions of the reader's context menu that the main window carries out. */
  function readerAction(a) {
    const v = a.vol;
    if (a.type === "search" || a.type === "phrase" || a.type === "book") {
      query = a.text;
      phrase = a.type === "phrase";
      rs.scope = "";
      rs.drill = "";
      if (a.type === "book") {
        rs.within = { lib: v.lib, books: v.book_id ? [v.book_id] : [], paths: v.book_id ? [] : [v.path], rel: null };
        rs.rangeLabel = v.title;
      } else {
        rs.within = null;
        rs.rangeLabel = "";
      }
      runSearch(1, v.lib && a.type === "book" ? v.lib : null);
    } else if (a.type === "quick") {
      quickScope = null;
      quickQuery = a.text;
      searchBox = false;
      recentBox = false;
      quick = true;
    } else {
      markAction(a);
    }
  }

  /** Bookmarks, collections and tags (added below). */
  function markAction(a) {
    notice = "这个功能还没有接上。";
  }

  let searchBox = $state(false); // full-text search popup
  let searchScope = $state(null); // { lib, label, within }: search from a node of the tree
  let quickScope = $state(null);
  function searchFrom(kind, scope) {
    if (kind === "title") {
      quickScope = scope;
      openQuickSearch(scope);
    } else openSearch(scope);
  }
  let searchInput = $state();
  let settingsBox = $state(false);
  let recentBox = $state(false); // reading history popup
  function openRecent() {
    searchBox = false;
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
    searchBox = false;
    recentBox = false;
    quickScope = scope && scope.lib ? scope : null; // a click passes the event
    quickQuery = null;
    quick = true;
  }
  function openSearch(scope = null) {
    searchScope = scope && scope.lib ? scope : null;
    quick = false;
    searchBox = true;
    tick().then(() => {
      searchInput?.focus();
      searchInput?.select();
    });
  }

  // window title: the document being read and its library
  $effect(() => {
    const c = workspace.ws.current;
    const lib = c ? libraries.find((l) => l.id === c.lib)?.title : "";
    const title = c ? `${c.title}${lib ? " · " + lib : ""} — Smart Library` : "Smart Library";
    document.title = title;
    getCurrentWindow().setTitle(title).catch(() => {});
  });

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
  const searchable = $derived(libraries.some((l) => l.index_ready));

  // props of the search results tab (mounted by the workspace)
  const rs = $state({
    result: null,
    within: null, // a tree node the search is limited to
    rangeLabel: "",
    scope: "",
    drill: "",
    onfacet: (root, name) => {
      rs.scope = `${root}/${name}`.replace(/^\//, "");
      rs.drill = "";
      runSearch(1, rs.result?.lib);
    },
    ondrill: (d) => {
      rs.drill = d.replace(/^\//, "");
      runSearch(rs.result?.page ?? 1, rs.result?.lib);
    },
    onclearscope: () => {
      rs.scope = "";
      rs.drill = "";
      rs.within = null;
      rs.rangeLabel = "";
      runSearch(1, rs.result?.lib);
    },
    onlib: (id) => {
      // facets differ between libraries: start again unscoped
      rs.scope = "";
      rs.drill = "";
      rs.within = null;
      rs.rangeLabel = "";
      runSearch(1, id);
    },
    onhit: (h, lb) =>
      workspace.openVolume({
        lib: h.lib,
        bookId: h.book_id,
        path: h.path,
        lb,
        query: lastSearch?.query ?? null,
        phrase: lastSearch?.phrase ?? false,
      }),
    onpage: (p) => runSearch(p, rs.result?.lib),
  });

  onMount(() => {
    refresh().then(() => st?.libraries.length && api.prepareTitles().catch(() => {}));
    window.addEventListener("keydown", shiftDown, true);
    window.addEventListener("keyup", shiftUp, true);
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
      window.removeEventListener("keydown", shiftDown, true);
      window.removeEventListener("keyup", shiftUp, true);
    };
  });

  $effect(() => {
    if (wsEl && st && !wsReady) {
      wsReady = true;
      workspace.init(wsEl, { resultsProps: rs, libraries: st.libraries, onlocate: locate, onaction: readerAction }).catch((e) => (error = String(e)));
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

  async function runSearch(page = 1, lib = null) {
    if (!query.trim()) return;
    searching = true;
    try {
      rs.result = await api.search({
        query,
        phrase,
        lib,
        scope: rs.scope || null,
        drill: rs.drill || null,
        within: rs.within,
        page,
        limit: 20,
      });
      lastSearch = { query, phrase };
      workspace.showResults();
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      searching = false;
    }
  }

  function submit(e) {
    e.preventDefault();
    if (!query.trim()) return;
    searchBox = false;
    rs.scope = "";
    rs.drill = "";
    rs.within = searchScope?.within ?? null;
    rs.rangeLabel = searchScope?.label ?? "";
    runSearch(1, searchScope?.lib ?? null);
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
            onsearchfrom={searchFrom} />
        {:else}
          <p class="muted">载入中…</p>
        {/if}
      </div>
    </nav>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="splitter" class:dragging role="separator" aria-orientation="vertical" aria-label="调整目录宽度"
      aria-valuenow={sideWidth} tabindex="0" onpointerdown={startDrag} onkeydown={splitterKey}
      ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>
    <main class="content" bind:this={wsEl}></main>
  </div>
  <footer class="status">
    <span class="muted">{buildingText}</span>
    <span class="grow"></span>
    <button class="link" onclick={() => (settingsBox = true)} title="设置（Ctrl+,）">设置</button>
    <button class="link" onclick={() => (showLog = !showLog)} title="本次运行期间的消息">消息{log.length ? ` (${log.length})` : ""}</button>
  </footer>
</div>

{#if searchBox}
  <div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (searchBox = false)}>
    <form class="searchbox" onsubmit={submit} onkeydown={(e) => e.key === "Escape" && (searchBox = false)}>
      {#if searchScope}
        <span class="chip" title="从目录树的这个节点搜索">范围：<b>{searchScope.label}</b>
          <button type="button" class="link" onclick={() => (searchScope = null)} aria-label="取消范围">×</button></span>
      {/if}
      <input type="search" placeholder="全文搜索：全部知识库（空格分隔关键词）" bind:value={query} bind:this={searchInput} aria-label="全文搜索" />
      <label class="check"><input type="checkbox" bind:checked={phrase} /> 精确短语</label>
      <button class="primary" disabled={searching || !searchable || !query.trim()} title={searchable ? "" : "请先为知识库建立索引"}>
        {searching ? "搜索中…" : "搜索"}
      </button>
    </form>
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

{#if settingsBox}
  <SettingsDialog onclose={() => (settingsBox = false)} />
{/if}

{#if recentBox}
  <RecentDialog onopen={openRecentItem} onclose={() => (recentBox = false)} />
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
  .status {
    display: flex; align-items: center; gap: 12px; padding: 2px 12px; min-height: 24px; font-size: 12px;
    border-top: 1px solid var(--line); background: var(--panel);
  }
  .backdrop {
    position: fixed; inset: 0; z-index: 900; background: rgba(0, 0, 0, 0.2);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 14vh;
  }
  .searchbox {
    display: flex; gap: 10px; align-items: center; width: min(720px, calc(100vw - 32px)); padding: 12px 16px;
    background: var(--paper); border-radius: 10px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  .chip { white-space: nowrap; font-size: 13px; color: var(--muted); }
  .chip b { color: var(--accent); }
  .searchbox input[type="search"] { flex: 1; min-width: 160px; font-size: 16px; }
  .check { display: flex; gap: 4px; align-items: center; white-space: nowrap; font-size: 14px; }
  .toast {
    position: fixed; right: 16px; bottom: 36px; z-index: 1000; max-width: min(520px, calc(100vw - 32px));
    display: flex; gap: 10px; align-items: flex-start; padding: 10px 14px; border-radius: 8px;
    background: var(--paper); border: 1px solid var(--line); box-shadow: 0 6px 24px rgba(0, 0, 0, 0.22);
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .toast.error { background: var(--hl-bg); color: var(--hl); border-color: var(--hl); }
  .log {
    position: fixed; right: 16px; bottom: 32px; z-index: 990; width: min(520px, calc(100vw - 32px)); max-height: 60vh;
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
