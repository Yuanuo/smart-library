<script>
  import { onMount, tick } from "svelte";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import * as api from "./lib/api.js";
  import Icon from "./lib/Icon.svelte";
  import DictReaderView from "./lib/DictReaderView.svelte";
  import { openDictEntryWindow } from "./lib/dictWindow.js";

  const PAGE_SIZE = 50;

  let { info = {} } = $props();
  // svelte-ignore state_referenced_locally
  let query = $state(info.query || "");
  let inputEl = $state();
  let listEl = $state();

  let dictState = $state(null);
  let scopeIds = $state(null);
  let showScope = $state(false);
  let scopeFilter = $state("");
  let draftScope = $state(new Set());

  let items = $state([]);
  let hasMore = $state(false);
  let totalEntries = $state(0);
  let elapsedMs = $state(0);
  let selectedIdx = $state(0);
  let currentDetail = $state(null);
  let loading = $state(false);
  let loadingMore = $state(false);
  let detailLoading = $state(false);
  let error = $state("");

  let seq = 0;
  let detailSeq = 0;
  let timer = null;

  const allDicts = $derived(dictState?.dicts ?? []);
  const activeScopeCount = $derived.by(() => {
    if (!allDicts.length) return 0;
    if (scopeIds) return scopeIds.length;
    return allDicts.filter((d) => d.enabled).length;
  });
  const filteredScopeDicts = $derived.by(() => {
    const q = scopeFilter.trim().toLowerCase();
    if (!q) return allDicts;
    return allDicts.filter((d) => d.name.toLowerCase().includes(q));
  });

  onMount(() => {
    const win = getCurrentWebviewWindow();
    const unlistenQuery = win.listen("dict-query", (e) => {
      const next = e.payload?.query ?? "";
      if (next) {
        query = next;
        runSearch();
      }
      inputEl?.focus();
      inputEl?.select();
    });

    (async () => {
      await tick();
      inputEl?.focus();
      inputEl?.select();
      try {
        dictState = await api.dictConfigGet();
      } catch (e) {
        error = String(e);
      }
      await runSearch();
    })();

    return () => {
      unlistenQuery.then((f) => f());
    };
  });

  function effectiveScope() {
    return scopeIds ? [...scopeIds] : null;
  }

  async function runSearch() {
    const my = ++seq;
    loading = true;
    loadingMore = false;
    error = "";
    try {
      const res = await api.dictSearch(query, effectiveScope(), 0, PAGE_SIZE);
      if (my !== seq) return;
      items = res.items;
      hasMore = res.has_more;
      totalEntries = res.total_entries;
      elapsedMs = res.elapsed_ms;
      selectedIdx = 0;
      listEl?.scrollTo(0, 0);
      if (res.items.length > 0) {
        await loadSingleEntry(res.items[0]);
      } else {
        currentDetail = null;
      }
    } catch (e) {
      if (my === seq) error = String(e);
    } finally {
      if (my === seq) loading = false;
    }
  }

  async function loadMore() {
    if (loading || loadingMore || !hasMore) return;
    const my = seq;
    loadingMore = true;
    try {
      const res = await api.dictSearch(query, effectiveScope(), items.length, PAGE_SIZE);
      if (my !== seq) return;
      items = [...items, ...res.items];
      hasMore = res.has_more;
    } catch (e) {
      if (my === seq) error = String(e);
    } finally {
      if (my === seq) loadingMore = false;
    }
  }

  function onListScroll() {
    if (!listEl || !hasMore || loadingMore || loading) return;
    if (listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - 40) {
      loadMore();
    }
  }

  async function loadSingleEntry(hit) {
    const my = ++detailSeq;
    detailLoading = true;
    try {
      const d = await api.dictEntry(hit.dict_id, hit.entry_id);
      if (my === detailSeq) currentDetail = d;
    } catch (e) {
      if (my === detailSeq) error = String(e);
    } finally {
      if (my === detailSeq) detailLoading = false;
    }
  }

  function selectHit(i) {
    if (!items[i]) return;
    selectedIdx = i;
    loadSingleEntry(items[i]);
  }

  function scheduleSearch() {
    clearTimeout(timer);
    timer = setTimeout(runSearch, 120);
  }

  function onInputKey(e) {
    if (e.key === "Escape" && showScope) {
      e.preventDefault();
      showScope = false;
    } else if (e.key === "Enter") {
      e.preventDefault();
      clearTimeout(timer);
      runSearch();
    } else if (e.key === "ArrowDown" && items.length) {
      e.preventDefault();
      const next = Math.min(items.length - 1, selectedIdx + 1);
      selectHit(next);
      listEl?.children[next]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp" && items.length) {
      e.preventDefault();
      const prev = Math.max(0, selectedIdx - 1);
      selectHit(prev);
      listEl?.children[prev]?.scrollIntoView({ block: "nearest" });
    }
  }

  function openScope() {
    if (showScope) {
      showScope = false;
      return;
    }
    draftScope = scopeIds
      ? new Set(scopeIds)
      : new Set(allDicts.filter((d) => d.enabled).map((d) => d.id));
    scopeFilter = "";
    showScope = true;
  }

  function toggleDraft(id) {
    const next = new Set(draftScope);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    draftScope = next;
  }

  function scopeSelectAll() {
    const next = new Set(draftScope);
    for (const d of filteredScopeDicts) next.add(d.id);
    draftScope = next;
  }

  function scopeClear() {
    const next = new Set(draftScope);
    for (const d of filteredScopeDicts) next.delete(d.id);
    draftScope = next;
  }

  function scopeInvert() {
    const next = new Set(draftScope);
    for (const d of filteredScopeDicts) {
      if (next.has(d.id)) next.delete(d.id);
      else next.add(d.id);
    }
    draftScope = next;
  }

  function applyScope() {
    scopeIds = [...draftScope];
    showScope = false;
    runSearch();
  }

  async function saveScopeAsDefault() {
    if (!dictState) return;
    const disabled = allDicts.filter((d) => !draftScope.has(d.id)).map((d) => d.id);
    try {
      dictState = await api.dictConfigSet(dictState.paths, disabled);
      scopeIds = null;
      showScope = false;
      runSearch();
    } catch (e) {
      error = String(e);
    }
  }

  async function reloadDicts() {
    try {
      dictState = await api.dictReload();
      runSearch();
    } catch (e) {
      error = String(e);
    }
  }

  function lookupTerm(term) {
    query = term;
    runSearch();
  }
</script>

<div class="dict-win">
  <div class="dict-head">
    <div class="dict-search-row">
      <input
        bind:this={inputEl}
        type="text"
        class="dict-input"
        bind:value={query}
        oninput={scheduleSearch}
        onkeydown={onInputKey}
        placeholder="输入词条…（支持 词* 前缀 / *词 后缀 / *词* 包含 / &quot;词&quot; 精确）"
      />
      <button type="button" class:on={showScope} onclick={openScope}>
        范围 ({activeScopeCount}/{allDicts.length})
      </button>
      <button type="button" onclick={reloadDicts} title="重新扫描已配置的词典目录">刷新词库</button>
    </div>
    <div class="dict-status muted">
      {#if !allDicts.length}
        <span>尚未配置词典，请在主窗口「设置 → 词典」中添加 .words 词典目录或文件。</span>
      {:else if loading}
        <span>正在检索…</span>
      {:else}
        <span>
          在 {totalEntries.toLocaleString()} 词条中查 · 已显示 {items.length}{hasMore ? "+" : ""} 项 · 用时 {elapsedMs} ms
        </span>
      {/if}
      {#if error}<span class="err">{error}</span>{/if}
    </div>
  </div>

  {#if showScope}
    <div class="scope-overlay">
      <div class="scope-bar">
        <input type="text" bind:value={scopeFilter} placeholder="过滤词典名称…" style="flex:1; min-width:140px" />
        <button type="button" onclick={scopeSelectAll}>全选</button>
        <button type="button" onclick={scopeClear}>清空</button>
        <button type="button" onclick={scopeInvert}>反选</button>
        <button type="button" onclick={saveScopeAsDefault}>应用并设为默认</button>
        <button type="button" class="primary" onclick={applyScope}>应用</button>
      </div>
      <div class="scope-grid">
        {#each filteredScopeDicts as d (d.id)}
          <label class="scope-item" title={d.path}>
            <input type="checkbox" checked={draftScope.has(d.id)} onchange={() => toggleDraft(d.id)} />
            <span class="scope-name">{d.name}</span>
            <span class="muted scope-size">{d.size.toLocaleString()}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}

  <div class="dict-body">
    <div class="dict-list" bind:this={listEl} onscroll={onListScroll} role="listbox" aria-label="词条列表">
      {#if !items.length}
        <div class="empty muted">
          {allDicts.length ? "没有匹配的词条" : "请先在主窗口设置中添加 .words 词典"}
        </div>
      {:else}
        {#each items as hit, i (hit.dict_id + ":" + hit.entry_id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="hit"
            class:selected={i === selectedIdx}
            class:row-current={i === selectedIdx}
            onclick={() => selectHit(i)}
            role="option"
            tabindex="0"
            aria-selected={i === selectedIdx}
          >
            <div class="hit-main">
              <span class="hit-title">{hit.title}</span>
              <span class="hit-dict muted">{hit.dict_name}</span>
            </div>
            <button
              type="button"
              class="pop-btn"
              title="在新窗口打开此词条"
              aria-label="在新窗口打开此词条"
              onclick={(e) => {
                e.stopPropagation();
                openDictEntryWindow({ dictId: hit.dict_id, entryId: hit.entry_id, title: hit.title });
              }}
            >
              <Icon name="open-window" size={14} />
            </button>
          </div>
        {/each}
        <div class="list-foot muted">
          {#if loadingMore}
            正在加载更多…
          {:else if hasMore}
            <button type="button" class="link" onclick={loadMore}>加载更多…</button>
          {:else}
            已显示全部 {items.length} 项
          {/if}
        </div>
      {/if}
    </div>

    <DictReaderView
      detail={currentDetail}
      loading={detailLoading}
      showPopout={true}
      onlookup={lookupTerm}
    />
  </div>
</div>

<style>
  .dict-win {
    height: 100vh;
    width: 100vw;
    display: flex;
    flex-direction: column;
    background: var(--paper);
    color: var(--ink);
    overflow: hidden;
  }
  .dict-head {
    padding: 10px 14px 8px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    flex: none;
  }
  .dict-search-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .dict-input {
    flex: 1;
    min-width: 180px;
    padding: 6px 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--paper);
    font-size: 14px;
  }
  .dict-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  button.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--paper);
  }
  .dict-status {
    margin-top: 6px;
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .err { color: var(--hl); }

  .scope-overlay {
    border-bottom: 1px solid var(--line);
    background: var(--bg);
    padding: 10px 14px;
    max-height: 250px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: none;
  }
  .scope-bar {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
  }
  .scope-bar input {
    padding: 4px 8px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--paper);
    font-size: 13px;
  }
  .scope-grid {
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 4px 12px;
  }
  .scope-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    cursor: pointer;
    padding: 2px 0;
  }
  .scope-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-size { font-size: 11px; flex: none; }

  .dict-body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .dict-list {
    width: 310px;
    flex: none;
    border-right: 1px solid var(--line);
    overflow-y: auto;
    background: var(--bg);
    padding: 4px;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 8px 7px 10px;
    border-radius: 6px;
    cursor: pointer;
  }
  .hit:hover { background: var(--accent-soft); }
  /* .row-current (app.css) marks the entry shown on the right */
  .hit-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .hit-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--ink);
    word-break: break-word;
  }
  .hit-dict {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pop-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--muted);
    border-radius: 5px;
    opacity: 0.65;
  }
  .hit:hover .pop-btn, .pop-btn:hover {
    opacity: 1;
    color: var(--accent);
    background: var(--paper);
    border-color: var(--line);
  }
  .list-foot {
    padding: 10px 8px;
    text-align: center;
    font-size: 12px;
  }
  .empty {
    padding: 32px 16px;
    text-align: center;
    font-size: 14px;
  }
</style>
