<script>
  // A full-text search page (one tab, or one window): the search box with
  // its match mode, the filters on the left (a tab per facet dimension with
  // check boxes, and the search range list that tree nodes are dragged into),
  // the hits with their snippets and the pager on the right. Changing the
  // words, the mode, the range or a filter searches again.
  //
  // `s` is the page's state, owned by the host (the workspace keeps it with
  // the tab, a window keeps it itself): { query, phrase, lib, filters,
  // scopes, tab, page, ran, autorun, rerun, focus }.
  import { onMount, untrack } from "svelte";
  import * as api from "./api.js";
  import { settings } from "./settings.svelte.js";
  import FacetPane from "./FacetPane.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";

  let { s, onopen, onstate = null, ontitle = null, onpopout = null, onai = null, onsearchresult = null } = $props();

  const DIMENSIONS = [
    ["/bulei", "部类"],
    ["/period", "时域"],
    ["/person", "人物"],
    ["/canon", "文集"],
    ["/kind", "典类"],
  ];

  let result = $state(null);
  let busy = $state(false);
  let error = $state("");
  let input = $state();
  let hitsEl = $state();
  let menu = $state(null);
  let ran = $state({ query: "", phrase: false }); // what the shown results were searched for
  let seq = 0;

  // ---------------------------------------------------------------- search
  async function run(page = 1) {
    const my = ++seq;
    busy = true;
    error = "";
    const filters = Object.entries(s.filters)
      .filter(([, v]) => v.length)
      .map(([root, values]) => ({ root, values }));
    try {
      const r = await api.search({
        query: s.query,
        phrase: s.phrase,
        lib: s.lib,
        filters: filters.length ? filters : null,
        withins: s.scopes.length ? $state.snapshot(s.scopes) : null,
        page,
        limit: 20,
      });
      if (my !== seq) return;
      result = r;
      s.lib = r.lib;
      s.page = r.page;
      s.ran = true;
      ran = { query: s.query.trim(), phrase: s.phrase };
      onsearchresult?.({
        query: ran.query || s.query,
        hits: r.hits || [],
        totalVolumes: r.total_volumes || 0,
        totalBooks: r.total_books || 0,
      });
      ontitle?.(title());
      if (hitsEl) hitsEl.scrollTop = 0;
    } catch (e) {
      if (my === seq) error = String(e);
    } finally {
      if (my === seq) busy = false;
      onstate?.();
    }
  }

  function title() {
    const q = s.query.trim();
    return q ? `搜索：${q.length > 12 ? q.slice(0, 12) + "…" : q}` : s.ran ? "搜索：全部" : "全文搜索";
  }

  function submit(e) {
    e?.preventDefault();
    run(1);
  }

  function setMode(phrase) {
    if (s.phrase === phrase) return;
    s.phrase = phrase;
    onstate?.();
    if (s.ran) run(1);
  }

  function changed() {
    onstate?.();
    if (s.ran) run(1);
  }

  // ---------------------------------------------------------------- filters
  const selected = (root) => s.filters[root] ?? [];
  function toggle(root, name) {
    const cur = new Set(selected(root));
    if (cur.has(name)) cur.delete(name);
    else cur.add(name);
    s.filters = { ...s.filters, [root]: [...cur] };
    changed();
  }
  function clearDim(root) {
    s.filters = { ...s.filters, [root]: [] };
    changed();
  }
  function clearFilters() {
    s.filters = {};
    changed();
  }
  const labelOf = (root) => DIMENSIONS.find((d) => d[0] === root)?.[1] ?? root;
  const chips = $derived(Object.entries(s.filters).flatMap(([root, vs]) => vs.map((name) => ({ root, name }))));

  function pickLibrary(id) {
    // the facets of another library are other values: start again
    s.lib = id;
    s.filters = {};
    changed();
  }

  // ---------------------------------------------------------------- range
  /** Items dragged or sent from the tree. A search is repeated when one has run. */
  function addScopes(items) {
    const have = new Set(s.scopes.map((x) => `${x.lib}|${x.label}`));
    const fresh = items.filter((x) => !have.has(`${x.lib}|${x.label}`));
    if (!fresh.length) return;
    s.scopes = [...s.scopes, ...fresh];
    s.tab = "scope";
    s.filters = {};
    changed();
  }
  function removeScope(i) {
    s.scopes = s.scopes.filter((_, j) => j !== i);
    s.filters = {};
    changed();
  }
  function clearScopes() {
    s.scopes = [];
    s.filters = {};
    changed();
  }
  function scopeMenu(e, i) {
    e.preventDefault();
    const items = [];
    if (i != null) items.push({ label: "移除此项", action: () => removeScope(i) });
    items.push({ label: "移除全部", action: clearScopes, disabled: !s.scopes.length });
    menu = { x: e.clientX, y: e.clientY, items };
  }

  // ---------------------------------------------------------------- tabs
  const tabs = $derived.by(() => {
    const list = [];
    for (const [root, label] of DIMENSIONS) {
      const f = result?.facets.find((x) => x.root === root);
      const n = selected(root).length;
      if (result && !f?.values.length && !n) continue;
      list.push({ id: root, label, n });
    }
    list.push({ id: "scope", label: "范围", n: s.scopes.length });
    return list;
  });
  const activeTab = $derived(tabs.some((t) => t.id === s.tab) ? s.tab : "scope");
  const facet = $derived(result?.facets.find((f) => f.root === activeTab));

  // ---------------------------------------------------------------- hits
  function open(h, lb) {
    onopen({
      lib: h.lib,
      bookId: h.book_id,
      path: h.path,
      lb,
      query: ran.query || null,
      phrase: ran.phrase,
    });
  }
  const browsing = $derived(result && !ran.query);

  // ---------------------------------------------------------------- side width
  let sideWidth = $state(300);
  try {
    sideWidth = Math.max(220, Number(localStorage.getItem("search.side")) || 300);
  } catch {}
  function dragSide(e) {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = sideWidth;
    const move = (ev) => (sideWidth = Math.round(Math.min(Math.max(220, w0 + ev.clientX - x0), 640)));
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      try {
        localStorage.setItem("search.side", String(sideWidth));
      } catch {}
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  // ---------------------------------------------------------------- life
  let lastRerun = untrack(() => s.rerun ?? 0);
  let lastFocus = untrack(() => s.focus ?? 0);
  $effect(() => {
    const n = s.rerun ?? 0; // the host changed the range (or filters): search again
    if (n !== lastRerun) {
      lastRerun = n;
      untrack(() => s.ran && run(1));
    }
  });
  $effect(() => {
    const n = s.focus ?? 0;
    if (n !== lastFocus) {
      lastFocus = n;
      untrack(() => {
        input?.focus();
        input?.select();
      });
    }
  });
  onMount(() => {
    ontitle?.(title());
    if (s.ran || s.autorun) {
      s.autorun = false;
      run(s.page || 1);
    } else input?.focus();
  });
</script>

<div class="page">
  <form class="bar" onsubmit={submit}>
    <input type="search" class="q" bind:this={input} bind:value={s.query} oninput={() => onstate?.()}
      placeholder="全文搜索（空格分隔关键词；不输入直接搜索，可浏览全部）" aria-label="全文搜索" />
    <span class="seg" role="radiogroup" aria-label="匹配方式">
      <button type="button" class:on={!s.phrase} onclick={() => setMode(false)} role="radio" aria-checked={!s.phrase}
        title="每个二字组都出现在同一篇文档即可，不要求连在一起">模糊</button>
      <button type="button" class:on={s.phrase} onclick={() => setMode(true)} role="radio" aria-checked={s.phrase}
        title="整句连续出现（忽略标点）">精确</button>
    </span>
    <button class="primary" disabled={busy}>{busy ? "搜索中…" : "搜索"}</button>
    {#if settings.aiOn && onai}
      <button type="button" class="ai-btn" onclick={() => onai({ toggle: true })} title="打开 / 收起右侧 AI 助手栏（F1，可根据搜索结果编写创新方案）">
        <Icon name="sparkles" size={16} /> AI 助手
      </button>
    {/if}
    {#if onpopout}
      <button type="button" class="link" onclick={onpopout} title="在新窗口打开这个搜索">新窗口</button>
    {/if}
  </form>

  <div class="main">
    <aside class="side" style="width: {sideWidth}px">
      <div class="tabs" role="tablist">
        {#each tabs as t (t.id)}
          <button role="tab" aria-selected={activeTab === t.id} class:on={activeTab === t.id} onclick={() => ((s.tab = t.id), onstate?.())}>
            {t.label}{#if t.id === "scope"}{t.n ? "*" : ""}{:else if t.n}<span class="dot">{t.n}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="panes">
        {#if activeTab === "scope"}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="scope" data-scope-drop onscopedrop={(e) => addScopes(e.detail)} oncontextmenu={(e) => scopeMenu(e, null)}>
            <p class="help muted">
              限定搜索范围，默认（空列表）为全部知识库。<br />
              添加：从左侧目录树拖动节点到下方列表，或在节点的右键菜单里选「加入搜索范围」。<br />
              移除：点 ×，或在列表里按右键。
            </p>
            <ul>
              {#each s.scopes as sc, i}
                <li oncontextmenu={(e) => (e.stopPropagation(), scopeMenu(e, i))} title={sc.label}>
                  <span class="lab">{sc.label}</span>
                  <button class="link x" onclick={() => removeScope(i)} aria-label="移除">×</button>
                </li>
              {:else}
                <li class="muted empty">（空：搜索全部）</li>
              {/each}
            </ul>
            {#if s.scopes.length}<button class="link clr" onclick={clearScopes}>清空范围</button>{/if}
          </div>
        {:else if facet}
          {#key facet.root}
            <FacetPane {facet} selected={selected(facet.root)} ontoggle={(n) => toggle(facet.root, n)} onclear={() => clearDim(facet.root)} />
          {/key}
        {:else}
          <p class="muted hint">点「搜索」后，这里列出可以用来过滤的{DIMENSIONS.map((d) => d[1]).join("、")}。</p>
        {/if}
      </div>
    </aside>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="split" role="separator" aria-orientation="vertical" tabindex="0" onpointerdown={dragSide}
      ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>

    <section class="results">
      {#if result && result.libraries.length > 1}
        <div class="libs" role="tablist" aria-label="知识库">
          {#each result.libraries as l}
            <button role="tab" class:on={l.id === result.lib} aria-selected={l.id === result.lib} onclick={() => pickLibrary(l.id)}>
              {l.title} <span class="n">{l.count}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if chips.length || s.scopes.length}
        <div class="chips">
          {#each chips as c (c.root + c.name)}
            <span class="chip">{labelOf(c.root)}：{c.name}
              <button class="link" onclick={() => toggle(c.root, c.name)} aria-label="取消">×</button></span>
          {/each}
          {#if s.scopes.length}
            <span class="chip scoped">范围：{s.scopes.length} 项
              <button class="link" onclick={() => ((s.tab = "scope"), onstate?.())} aria-label="查看范围">…</button></span>
          {/if}
          {#if chips.length}<button class="link" onclick={clearFilters}>清除过滤</button>{/if}
        </div>
      {/if}
      <div class="hits" bind:this={hitsEl}>
        {#if error}
          <p class="err">{error}</p>
        {:else if !result}
          <p class="muted intro">
            输入关键词后按 Enter；也可以不输入，直接点「搜索」浏览全部，再用左边的过滤和范围慢慢缩小。
          </p>
        {:else}
          <div class="summary muted">
            {browsing ? "全部 " : ""}共 {result.total_volumes} 篇，{result.total_books} 部 ·{result.millis.toFixed(0)} ms
            {#if result.pages > 1}· 第 {result.page} / {result.pages} 页{/if}
          </div>
          {#each result.hits as h}
            <article>
              <h2>
                <button class="link" onclick={() => open(h, null)}>{h.title} {h.book_id ? h.subtitle : ""}</button>
                <span class="muted id">{[h.book_id, h.book_id ? "" : h.subtitle].filter(Boolean).join(" · ")}{browsing ? "" : ` · ${h.occurrences} 处`}</span>
              </h2>
              {#each h.snippets as sn}
                <button class="snippet" onclick={() => open(h, sn.lb)}>
                  <span class="where muted">{sn.lb} 第{sn.pos}字</span>
                  <span>…{#each sn.parts as p}{#if p.hl}<span class="hl">{p.t}</span>{:else}{p.t}{/if}{/each}…</span>
                </button>
              {/each}
            </article>
          {:else}
            <p class="muted">没有找到。模糊匹配要求每个二字组都出现在同一篇文档；可以用空格分开关键词，或去掉一些过滤。</p>
          {/each}
          {#if result.pages > 1}
            <nav class="pager">
              <button disabled={result.page <= 1 || busy} onclick={() => run(1)}>首页</button>
              <button disabled={result.page <= 1 || busy} onclick={() => run(result.page - 1)}>上一页</button>
              <span class="muted">
                <input class="jump" type="number" min="1" max={result.pages} value={result.page}
                  onchange={(e) => run(Math.min(Math.max(1, Number(e.currentTarget.value) || 1), result.pages))} /> / {result.pages}
              </span>
              <button disabled={result.page >= result.pages || busy} onclick={() => run(result.page + 1)}>下一页</button>
              <button disabled={result.page >= result.pages || busy} onclick={() => run(result.pages)}>末页</button>
            </nav>
          {/if}
        {/if}
      </div>
    </section>
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--paper); }
  .bar { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .q { flex: 1; min-width: 120px; font-size: 16px; }
  .seg { display: inline-flex; flex: none; }
  .seg button { border-radius: 0; padding: 3px 12px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; }
  .seg button:last-child { border-radius: 0 6px 6px 0; margin-left: -1px; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .ai-btn {
    display: inline-flex; align-items: center; gap: 4px; padding: 3px 10px;
    font-size: 13px; border-radius: 6px; flex: none;
  }
  .ai-btn:hover { background: var(--accent-soft); border-color: var(--accent); }
  .main { flex: 1; display: flex; min-height: 0; }
  .side { flex: none; display: flex; flex-direction: column; background: var(--panel); min-height: 0; min-width: 0; }
  .split { flex: none; width: 5px; cursor: col-resize; border-left: 1px solid var(--line); background: var(--panel); touch-action: none; }
  .split:hover, .split:focus-visible { background: var(--accent-soft); outline: none; }
  .tabs { display: flex; flex-wrap: wrap; gap: 2px 4px; padding: 6px 8px 0; border-bottom: 1px solid var(--line); }
  .tabs button {
    border: none; background: none; padding: 5px 8px; color: var(--accent); font-size: 14px;
    border-bottom: 2px solid transparent; border-radius: 0; opacity: 0.8;
  }
  .tabs button.on { border-bottom-color: var(--accent); opacity: 1; font-weight: 600; }
  .tabs button:hover { background: var(--accent-soft); }
  .dot { margin-left: 3px; font-size: 11px; background: var(--accent); color: var(--paper); border-radius: 8px; padding: 0 5px; }
  .panes { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .panes > :global(*) { flex: 1; min-height: 0; }
  .hint { padding: 12px; }
  .scope { padding: 8px 10px; overflow: auto; display: block; border: 2px dashed transparent; }
  .scope:global(.scope-over) { border-color: var(--accent); background: var(--accent-soft); }
  .help { font-size: 13px; line-height: 1.7; margin: 0 0 6px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
  .scope ul { list-style: none; margin: 0; padding: 0; }
  .scope li { display: flex; align-items: center; gap: 6px; padding: 4px 0; border-bottom: 1px dashed var(--line); font-size: 14px; }
  .scope li .lab { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scope li.empty { border: none; }
  .x { font-size: 16px; color: var(--muted); }
  .clr { margin-top: 6px; }
  .results { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .libs { display: flex; gap: 6px; flex-wrap: wrap; padding: 8px 24px 0; }
  .libs button { border-radius: 14px; font-size: 13px; padding: 2px 12px; }
  .libs button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .libs .n { opacity: 0.75; margin-left: 4px; }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; padding: 8px 24px 0; font-size: 13px; }
  .chip { background: var(--accent-soft); border-radius: 12px; padding: 1px 4px 1px 10px; }
  .chip .link { padding: 0 4px; }
  .hits { flex: 1; overflow: auto; padding: 8px 24px 40px; }
  .intro, .err { padding-top: 8px; }
  .err { color: var(--hl); white-space: pre-wrap; }
  .summary { font-size: 13px; margin-bottom: 8px; }
  article { border-bottom: 1px solid var(--line); padding: 10px 0 12px; }
  h2 { font-size: 18px; margin: 0 0 6px; font-weight: 600; display: flex; gap: 10px; align-items: baseline; }
  .id { font-size: 13px; font-weight: normal; }
  .snippet {
    display: flex; gap: 12px; width: 100%; text-align: left; border: none; background: none;
    padding: 4px 6px; line-height: 1.8; font-size: 16px; border-radius: 4px;
  }
  .snippet:hover { background: var(--accent-soft); }
  .where { flex: none; width: 110px; font-size: 12px; font-family: ui-monospace, Consolas, monospace; padding-top: 4px; }
  .pager { display: flex; gap: 10px; align-items: center; justify-content: center; padding: 16px; }
  .jump { width: 56px; text-align: center; }
</style>
