<script>
  import { onMount } from "svelte";
  import * as api from "./lib/api.js";
  import * as workspace from "./lib/workspace.svelte.js";
  import Tree from "./lib/Tree.svelte";

  let st = $state(null);
  let error = $state("");
  let pathInput = $state("");

  let treeId = $state(null);
  let nodes = $state([]);
  let treeLoading = $state(false);

  let query = $state("");
  let phrase = $state(false);
  let searching = $state(false);
  let lastSearch = null; // query of the shown results, used for highlights

  let progress = $state(null);
  let indexMessage = $state("");

  let wsEl = $state();
  let wsReady = false;

  // props of the search results tab (mounted by the workspace)
  const rs = $state({
    result: null,
    scope: "",
    drill: "",
    onfacet: (root, name) => {
      rs.scope = `${root}/${name}`.replace(/^\//, "");
      rs.drill = "";
      runSearch(1);
    },
    ondrill: (d) => {
      rs.drill = d.replace(/^\//, "");
      runSearch(rs.result?.page ?? 1);
    },
    onclearscope: () => {
      rs.scope = "";
      rs.drill = "";
      runSearch(1);
    },
    onhit: (h, lb) =>
      workspace.openVolume({
        bookId: h.book_id,
        path: h.path,
        lb,
        query: lastSearch?.query ?? null,
        phrase: lastSearch?.phrase ?? false,
      }),
    onpage: (p) => runSearch(p),
  });

  onMount(() => {
    refresh();
    const un1 = api.onIndexProgress((p) => (progress = p));
    const un2 = api.onIndexDone((d) => {
      progress = null;
      indexMessage = d.message;
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

  // the workspace needs an open bookcase (tabs restore volumes)
  $effect(() => {
    if (wsEl && st?.data_version && !wsReady) {
      wsReady = true;
      workspace.init(wsEl, { resultsProps: rs }).catch((e) => (error = String(e)));
    }
  });

  async function refresh() {
    try {
      st = await api.status();
      error = st.error || "";
      if (!treeId && st.default_tree) await loadTree(st.default_tree);
    } catch (e) {
      error = String(e);
    }
  }

  async function chooseBookcase() {
    try {
      st = await api.openBookcase(pathInput);
      error = "";
      treeId = null;
      if (st.default_tree) await loadTree(st.default_tree);
    } catch (e) {
      error = String(e);
    }
  }

  async function loadTree(id) {
    treeId = id;
    treeLoading = true;
    try {
      nodes = await api.tree(id);
    } catch (e) {
      error = String(e);
    } finally {
      treeLoading = false;
    }
  }

  async function openNode(node) {
    try {
      await workspace.openVolume({ bookId: node.book_id, path: node.start });
    } catch (e) {
      error = String(e);
    }
  }

  async function runSearch(page = 1) {
    if (!query.trim()) return;
    searching = true;
    try {
      rs.result = await api.search({
        query,
        phrase,
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
    rs.drill = "";
    runSearch(1);
  }

  async function startIndex() {
    indexMessage = "";
    try {
      progress = { done: 0, total: 1 };
      await api.buildIndex();
    } catch (e) {
      progress = null;
      error = String(e);
    }
  }

  const indexStale = $derived(
    st?.index_ready && st.index_data_version && st.data_version && st.index_data_version !== st.data_version,
  );
</script>

<div class="app">
  <header class="top">
    <div class="brand">Smart Library</div>
    {#if st?.data_version}
      <form class="search" onsubmit={submit}>
        <input type="search" placeholder="搜索经文（空格分隔关键词）" bind:value={query} aria-label="搜索" />
        <label class="check"><input type="checkbox" bind:checked={phrase} /> 精确短语</label>
        <button class="primary" disabled={searching || !st.index_ready}>{searching ? "搜索中…" : "搜索"}</button>
      </form>
      <div class="index muted">
        {#if progress}
          建立索引 {progress.done}/{progress.total}
          <progress max={progress.total} value={progress.done}></progress>
        {:else if !st.index_ready}
          <button onclick={startIndex}>建立索引</button>
        {:else}
          CBETA {st.data_version}
          {#if indexStale}<button onclick={startIndex} title="索引是 {st.index_data_version} 的">重建索引</button>{/if}
        {/if}
        {#if indexMessage}<span>· {indexMessage}</span>{/if}
      </div>
    {/if}
  </header>

  {#if error}
    <div class="error" role="alert">{error} <button class="link" onclick={() => (error = "")}>关闭</button></div>
  {/if}

  {#if st && !st.data_version}
    <main class="setup">
      <h1>打开 CBETA 数据包</h1>
      <p class="muted">请输入 bookcase.zip 的完整路径（或已解压的目录）。Smart Library 只读取，不会修改它。</p>
      <form onsubmit={(e) => { e.preventDefault(); chooseBookcase(); }}>
        <input type="text" bind:value={pathInput} placeholder="C:\…\bookcase.zip" size="60" />
        <button class="primary">打开</button>
      </form>
    </main>
  {:else if st}
    <div class="body">
      <nav class="side">
        <select aria-label="目录树" value={treeId} onchange={(e) => loadTree(e.target.value)}>
          {#each st.trees as t}<option value={t.id}>{t.title}</option>{/each}
        </select>
        <div class="tree">
          {#if treeLoading}<p class="muted">载入中…</p>{:else}
            <Tree {nodes} onopen={openNode} />
          {/if}
        </div>
      </nav>
      <main class="content" bind:this={wsEl}></main>
    </div>
  {:else}
    <main class="setup muted">载入中…</main>
  {/if}
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  .top {
    display: flex; align-items: center; gap: 16px; padding: 8px 16px;
    border-bottom: 1px solid var(--line); background: var(--panel);
  }
  .brand { font-weight: 700; color: var(--accent); letter-spacing: 0.05em; white-space: nowrap; }
  .search { display: flex; gap: 8px; align-items: center; flex: 1; max-width: 720px; }
  .search input[type="search"] { flex: 1; min-width: 160px; }
  .check { display: flex; gap: 4px; align-items: center; white-space: nowrap; font-size: 14px; }
  .index { font-size: 13px; display: flex; gap: 8px; align-items: center; white-space: nowrap; margin-left: auto; }
  .error {
    padding: 6px 16px; background: var(--hl-bg); color: var(--hl); font-size: 14px;
    border-bottom: 1px solid var(--line);
  }
  .body { flex: 1; display: flex; min-height: 0; }
  .side {
    width: 300px; flex: none; display: flex; flex-direction: column; gap: 8px; padding: 10px 8px;
    border-right: 1px solid var(--line); background: var(--panel); min-height: 0;
  }
  .side select { width: 100%; }
  .tree { flex: 1; overflow: auto; font-size: 15px; }
  .content { flex: 1; min-width: 0; min-height: 0; position: relative; }
  .setup { padding: 64px; max-width: 820px; }
  .setup form { display: flex; gap: 8px; margin-top: 16px; }
  .setup input { flex: 1; }
</style>
