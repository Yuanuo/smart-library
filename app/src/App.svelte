<script>
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api.js";
  import * as workspace from "./lib/workspace.svelte.js";
  import LibraryPanel from "./lib/LibraryPanel.svelte";
  import BindDialog from "./lib/BindDialog.svelte";

  let st = $state(null);
  let error = $state("");
  let notice = $state("");

  let query = $state("");
  let phrase = $state(false);
  let searching = $state(false);
  let lastSearch = null; // query of the shown results, used for highlights

  let progress = $state({}); // lib id -> { done, total }
  let binding = $state(null); // null, or { path } for the bind dialog

  let wsEl = $state();
  let wsReady = false;

  const libraries = $derived(st?.libraries ?? []);
  const searchable = $derived(libraries.some((l) => l.index_ready));

  // props of the search results tab (mounted by the workspace)
  const rs = $state({
    result: null,
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
      runSearch(1, rs.result?.lib);
    },
    onlib: (id) => {
      // facets differ between libraries: start again unscoped
      rs.scope = "";
      rs.drill = "";
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
    refresh();
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
    };
  });

  $effect(() => {
    if (wsEl && st && !wsReady) {
      wsReady = true;
      workspace.init(wsEl, { resultsProps: rs, libraries: st.libraries }).catch((e) => (error = String(e)));
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

  function bound(newStatus) {
    const before = new Set(libraries.map((l) => l.id));
    st = newStatus;
    binding = null;
    const added = newStatus.libraries.filter((l) => !before.has(l.id));
    notice = added.length
      ? `已绑定：${added.map((l) => l.title).join("、")}。右键知识库可以建立索引。`
      : "目录树已更新。";
  }

  async function unbind(l) {
    const ok = await ask(`解除绑定「${l.title}」？\n它的索引会被删除；数据包本身不受影响，以后可以重新绑定。`, {
      title: "解除绑定",
      kind: "warning",
      okLabel: "解除绑定",
      cancelLabel: "取消",
    });
    if (!ok) return;
    try {
      await workspace.closeLibrary(l.id);
      st = await api.unbindLibrary(l.id);
      notice = `已解除绑定：${l.title}`;
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
    rs.scope = "";
    rs.drill = "";
    runSearch(1, null);
  }
</script>

<div class="app">
  <header class="top">
    <div class="brand">Smart Library</div>
    <form class="search" onsubmit={submit}>
      <input type="search" placeholder="搜索全部知识库（空格分隔关键词）" bind:value={query} aria-label="搜索" />
      <label class="check"><input type="checkbox" bind:checked={phrase} /> 精确短语</label>
      <button class="primary" disabled={searching || !searchable} title={searchable ? "" : "请先为知识库建立索引"}>
        {searching ? "搜索中…" : "搜索"}
      </button>
    </form>
  </header>

  {#if error}
    <div class="msg error" role="alert">{error} <button class="link" onclick={() => (error = "")}>关闭</button></div>
  {:else if notice}
    <div class="msg" role="status">{notice} <button class="link" onclick={() => (notice = "")}>关闭</button></div>
  {/if}

  <div class="body">
    <nav class="side" aria-label="知识库目录">
      <div class="side-head">
        <span class="muted">知识库</span>
        <button class="link" onclick={() => (binding = { path: "" })} title="绑定知识库">＋ 绑定</button>
      </div>
      <div class="tree">
        {#if st}
          <LibraryPanel
            {libraries}
            {progress}
            onopen={openNode}
            onbind={() => (binding = { path: "" })}
            onrebind={(l) => (binding = { path: l.source })}
            onunbind={unbind}
            onindex={startIndex} />
        {:else}
          <p class="muted">载入中…</p>
        {/if}
      </div>
    </nav>
    <main class="content" bind:this={wsEl}></main>
  </div>
</div>

{#if binding}
  <BindDialog initialPath={binding.path} onclose={() => (binding = null)} onbound={bound} />
{/if}

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  .top {
    display: flex; align-items: center; gap: 16px; padding: 8px 16px;
    border-bottom: 1px solid var(--line); background: var(--panel);
  }
  .brand { font-weight: 700; color: var(--accent); letter-spacing: 0.05em; white-space: nowrap; }
  .search { display: flex; gap: 8px; align-items: center; flex: 1; max-width: 760px; }
  .search input[type="search"] { flex: 1; min-width: 160px; }
  .check { display: flex; gap: 4px; align-items: center; white-space: nowrap; font-size: 14px; }
  .msg { padding: 6px 16px; background: var(--accent-soft); font-size: 14px; border-bottom: 1px solid var(--line); }
  .msg.error { background: var(--hl-bg); color: var(--hl); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side {
    width: 300px; flex: none; display: flex; flex-direction: column; gap: 6px; padding: 8px 8px;
    border-right: 1px solid var(--line); background: var(--panel); min-height: 0;
  }
  .side-head { display: flex; justify-content: space-between; align-items: center; padding: 0 4px; font-size: 13px; }
  .tree { flex: 1; overflow: auto; font-size: 15px; }
  .content { flex: 1; min-width: 0; min-height: 0; position: relative; }
</style>
