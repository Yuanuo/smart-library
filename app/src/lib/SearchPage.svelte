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
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import { settings } from "./settings.svelte.js";
  import FacetPane from "./FacetPane.svelte";
  import LibraryLocationDialog from "./LibraryLocationDialog.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";
  import { addScopesToSearch, scopesForLibrary, searchScope, setLibrarySearchScopes } from "./searchScope.js";
  import { searchCitation } from "./clip.js";

  let { s, onopen, onstate = null, ontitle = null, onpopout = null, onai = null, onsearchresult = null, onsetup = null } = $props();
  const NO_INDEX = "还没有可搜索的知识库：请先为知识库建立索引";

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
  let chooseScope = $state(null);
  let unavailable = $state(null);
  let selecting = $state(false);
  let picked = $state([]);
  let sharing = $state(false);
  let shareMessage = $state("");
  let ran = $state({ query: "", phrase: false }); // what the shown results were searched for
  let seq = 0;

  // ---------------------------------------------------------------- search
  async function run(page = 1) {
    const my = ++seq;
    busy = true;
    error = "";
    unavailable = null;
    const filters = Object.entries(s.filters)
      .filter(([, v]) => v.length)
      .map(([root, values]) => ({ root, values }));
    const query = s.query;
    const phrase = s.phrase;
    const library = s.libraryPinned || filters.length ? s.lib : null;
    const scopes = s.scopes.length ? $state.snapshot(s.scopes) : null;
    try {
      const r = await api.search({
        query,
        phrase,
        lib: library,
        filters: filters.length ? filters : null,
        withins: scopes,
        page,
        limit: 20,
      });
      if (my !== seq) return;
      result = r;
      if (library != null && library !== r.lib) {
        s.libraryPinned = false;
        s.filters = {};
      }
      s.lib = r.lib;
      s.page = r.page;
      s.ran = true;
      ran = { query: query.trim(), phrase };
      onsearchresult?.({
        query: ran.query,
        hits: r.hits || [],
        totalVolumes: r.total_volumes || 0,
        totalBooks: r.kind === "cbeta" ? r.total_books : null,
      });
      ontitle?.(title());
      if (hitsEl) hitsEl.scrollTop = 0;
    } catch (e) {
      if (my === seq) {
        error = String(e);
        onsearchresult?.(null);
        if (error === NO_INDEX) {
          const status = await api.status().catch(() => null);
          if (my === seq && status) {
            unavailable = { empty: !status.libraries.length, libraries: status.libraries };
          }
        }
      }
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
    if (ran.query !== s.query.trim()) clearPicked();
    run(1);
  }

  function setMode(phrase) {
    if (s.phrase === phrase) return;
    clearPicked();
    s.phrase = phrase;
    onstate?.();
    if (s.ran) run(1);
  }

  function changed(keepPicked = false) {
    if (!keepPicked) clearPicked();
    onstate?.();
    if (s.ran || busy || error) run(1);
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
    if (s.libraryPinned === (id != null) && (id == null || id === s.lib)) return;
    const keepPicked = !chips.length;
    if (id != null) s.lib = id;
    s.libraryPinned = id != null;
    s.filters = {};
    s.scopeFeedback = "";
    changed(keepPicked);
  }

  // ---------------------------------------------------------------- range
  const currentLibrary = $derived(result?.libraries.find((library) => library.id === s.lib));
  const currentScopes = $derived(scopesForLibrary(s.scopes, s.lib));

  function addScopes(items) {
    if (addScopesToSearch(s, items)) changed();
    else onstate?.();
  }
  function removeScope(scope) {
    if (currentScopes.includes(scope)) updateScopes(currentScopes.filter((item) => item !== scope));
  }
  function updateScopes(scopes) {
    setLibrarySearchScopes(s, scopes);
    changed();
  }
  function scopeMenu(e, scope) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, items: [{ label: "移除此项", action: () => removeScope(scope) }] };
  }

  // ---------------------------------------------------------------- tabs
  const switchingLibrary = $derived(busy && result && result.lib !== s.lib);
  const tabs = $derived.by(() => {
    const list = [];
    for (const [root, label] of DIMENSIONS) {
      const f = switchingLibrary ? null : result?.facets.find((x) => x.root === root);
      const n = selected(root).length;
      if (result && !f?.values.length && !n) continue;
      list.push({ id: root, label, n });
    }
    list.push({ id: "scope", label: "范围", n: currentScopes.length });
    return list;
  });
  const activeTab = $derived(tabs.some((t) => t.id === s.tab) ? s.tab : "scope");
  const facet = $derived(switchingLibrary ? null : result?.facets.find((f) => f.root === activeTab));

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
  const totalMatches = $derived(result?.libraries.reduce((total, library) => total + library.count, 0) ?? 0);
  const otherMatch = $derived(result?.libraries.find((library) => library.id !== result.lib && library.count > 0));

  const hitKey = (hit) => JSON.stringify([hit.lib, hit.path]);
  const pickedKeys = $derived(new Set(picked.map((item) => hitKey(item.hit))));
  const allPagePicked = $derived(!!result?.hits.length && result.hits.every((hit) => pickedKeys.has(hitKey(hit))));

  function clearPicked() {
    picked = [];
    shareMessage = "";
  }

  function pickHits(hits, checked) {
    const keys = new Set(hits.map(hitKey));
    if (!checked) {
      picked = picked.filter((item) => !keys.has(hitKey(item.hit)));
    } else {
      const libraryTitle = result.libraries.find((item) => item.id === result.lib)?.title || "";
      picked = [...picked, ...hits.filter((hit) => !pickedKeys.has(hitKey(hit))).map((hit) => ({ hit, libraryTitle }))];
    }
    shareMessage = "";
  }

  async function shareCitations(toFile = false) {
    if (!picked.length || sharing) return;
    const items = $state.snapshot(picked);
    sharing = true;
    shareMessage = "";
    try {
      let path;
      if (toFile) {
        path = await saveDialog({ defaultPath: "搜索引用.md", filters: [{ name: "Markdown", extensions: ["md"] }] });
        if (!path) return;
      }
      const citations = await Promise.all(items.map(({ hit, libraryTitle }) => searchCitation(hit, libraryTitle)));
      const markdown = citations.join("\n") + "\n";
      if (path) await api.saveBytes(path, new TextEncoder().encode(markdown));
      else await navigator.clipboard.writeText(markdown);
      shareMessage = `${path ? "已导出" : "已复制"} ${citations.length} 条引用`;
    } catch (e) {
      shareMessage = `无法${toFile ? "导出" : "复制"}引用：${String(e)}`;
    } finally {
      sharing = false;
    }
  }

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

  function resizeSide(e) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    sideWidth = Math.min(640, Math.max(220, sideWidth + (e.key === "ArrowRight" ? 16 : -16)));
  }

  // ---------------------------------------------------------------- life
  let lastRerun = untrack(() => s.rerun ?? 0);
  let lastFocus = untrack(() => s.focus ?? 0);
  $effect(() => {
    const n = s.rerun ?? 0; // the host changed the range (or filters): search again
    if (n !== lastRerun) {
      lastRerun = n;
      untrack(() => changed());
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
    if (s.focusOnOpen || !s.ran && !s.autorun) input?.focus();
    if (s.ran || s.autorun) {
      s.autorun = false;
      run(s.page || 1);
    }
    const off = api.onIndexDone((event) => {
      if (event.ok && (unavailable || error === NO_INDEX)) run(1);
    });
    return () => off.then((unlisten) => unlisten());
  });
</script>

<div class="page">
  <form class="bar" onsubmit={submit}>
    <input type="search" class="q" bind:this={input} bind:value={s.query} oninput={() => onstate?.()}
      placeholder="输入关键词，留空浏览全部" aria-label="全文搜索" />
    <span class="seg" role="radiogroup" aria-label="匹配方式">
      <button type="button" class:on={!s.phrase} onclick={() => setMode(false)} role="radio" aria-checked={!s.phrase}
        title="中文按相邻两字分组，各组可出现在同一文档的不同位置。用空格分隔关键词。">宽松匹配</button>
      <button type="button" class:on={s.phrase} onclick={() => setMode(true)} role="radio" aria-checked={s.phrase}
        title="整句连续出现（忽略标点）">精确短语</button>
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
          <div class="scope" data-scope-drop data-scope-lib={s.lib} onscopedrop={(e) => addScopes(e.detail)}>
            <div class="scope-actions">
              <button type="button" disabled={!currentLibrary || !!error} onclick={() => (chooseScope = currentLibrary.id)}>添加范围…</button>
              <button type="button" disabled={!currentScopes.length} onclick={() => updateScopes([])}>清空范围</button>
            </div>
            {#if s.scopeFeedback}<p class="scope-note" role="status">{s.scopeFeedback}</p>{/if}
            <ul>
              {#each currentScopes as sc (sc)}
                <li oncontextmenu={(e) => scopeMenu(e, sc)} title={sc.label}>
                  <span class="lab">{sc.label}</span>
                  <button type="button" class="link x" onclick={() => removeScope(sc)} aria-label={`移除：${sc.label}`}>×</button>
                </li>
              {:else}
                <li class="muted empty">{currentLibrary ? `在「${currentLibrary.title}」中搜索全部文档。可拖入当前库的目录或文档。` : "搜索后可为当前知识库添加范围，也可直接拖入目录树中的节点。"}</li>
              {/each}
            </ul>
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
    <div class="split" role="separator" aria-orientation="vertical" aria-label="调整筛选栏宽度" aria-valuenow={sideWidth} tabindex="0" onpointerdown={dragSide} onkeydown={resizeSide}
      ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>

    <section class="results">
      {#if result && !error && result.libraries.length > 1}
        <div class="libs" role="group" aria-label="显示哪个知识库的结果">
          <button aria-pressed={!s.libraryPinned} class:on={!s.libraryPinned} onclick={() => pickLibrary(null)}>自动选库</button>
          {#each result.libraries as l}
            <button class:on={s.libraryPinned && l.id === s.lib} aria-pressed={s.libraryPinned && l.id === s.lib} onclick={() => pickLibrary(l.id)}>
              {l.title} <span class="n">{l.count}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if chips.length}
        <div class="chips">
          {#each chips as c (c.root + c.name)}
            <span class="chip">{labelOf(c.root)}：{c.name}
              <button class="link" onclick={() => toggle(c.root, c.name)} aria-label="取消">×</button></span>
          {/each}
          <button class="link" onclick={clearFilters}>清除过滤</button>
        </div>
      {/if}
      <div class="hits" bind:this={hitsEl}>
        {#if unavailable}
          <div class="empty-state">
            {#if unavailable.empty}
              <p>先添加资料或新建笔记，全文索引会在后台自动建立。</p>
              {#if onsetup}
                <button class="primary" type="button" onclick={() => onsetup({ type: "folder" })}>添加本地资料…</button>
                <button type="button" onclick={() => onsetup({ type: "note" })}>新建笔记</button>
              {/if}
            {:else}
              <p>尚无可用的全文索引。</p>
              {#each unavailable.libraries as item (item.id)}
                <div class="availability">
                  <strong>{item.title}</strong>
                  {#if item.error}<span class="muted" title={item.error}>资料不可用</span>
                  {:else if item.building}<span class="muted">正在建立索引…</span>
                  {:else if onsetup}<button type="button" onclick={() => onsetup({ type: "index", id: item.id })}>建立索引</button>{/if}
                </div>
              {/each}
            {/if}
          </div>
        {:else if error}
          <p class="err">{error}</p>
        {:else if switchingLibrary}
          <p class="muted" role="status">正在载入该知识库的结果…</p>
        {:else if !result}
          <p class="muted intro">
            输入关键词后按 Enter。留空搜索可以浏览当前范围内的文档。
          </p>
        {:else}
          <div class="summary muted">
            {#if result.libraries.length > 1}
              共 {totalMatches} 个命中文档{chips.length ? "（筛选前）" : ""} · {result.libraries.find((l) => l.id === result.lib)?.title}显示
            {:else}
              {browsing ? "全部 " : "命中 "}
            {/if}
            {#if result.kind === "cbeta"}{result.total_books} 部、{result.total_volumes} 卷{:else}{result.total_volumes} 篇{/if}
            {#if result.pages > 1}· 第 {result.page} / {result.pages} 页{/if}
          </div>
          {#if result.unavailable_libraries.length}
            <p class="muted unsearched">未参与全文搜索：{result.unavailable_libraries.join("、")}</p>
          {/if}
          {#if result.hits.length || picked.length}
            <div class="citation-actions">
              {#if selecting}
                {#if result.hits.length}
                  <label><input type="checkbox" checked={allPagePicked} onchange={(event) => pickHits(result.hits, event.currentTarget.checked)} />全选本页</label>
                {/if}
                <button type="button" onclick={() => shareCitations()} disabled={!picked.length || sharing}>复制引用（{picked.length}）</button>
                <button type="button" onclick={() => shareCitations(true)} disabled={!picked.length || sharing}>导出 Markdown…</button>
                <button type="button" class="link" onclick={() => { selecting = false; clearPicked(); }} disabled={sharing}>退出选择</button>
              {:else}
                <button type="button" class="link" onclick={() => (selecting = true)}>选择结果</button>
              {/if}
              {#if shareMessage}<span class="muted" role="status">{shareMessage}</span>{/if}
            </div>
          {/if}
          {#each result.hits as h}
            <article>
              <h2>
                {#if selecting}<input type="checkbox" checked={pickedKeys.has(hitKey(h))} aria-label={`选择 ${h.title} ${h.subtitle}`}
                  onchange={(event) => pickHits([h], event.currentTarget.checked)} />{/if}
                <button class="link" onclick={() => open(h, null)}>{h.title} {h.book_id ? h.subtitle : ""}</button>
                <span class="muted id">{[h.book_id, h.book_id ? "" : h.subtitle, !browsing && h.occurrences ? `${h.occurrences} 处` : ""].filter(Boolean).join(" · ")}</span>
              </h2>
              {#each h.snippets as sn}
                <button class="snippet" onclick={() => open(h, sn.lb)}>
                  <span class="where muted">{sn.lb} 第{sn.pos}字</span>
                  <span>…{#each sn.parts as p}{#if p.hl}<span class="hl">{p.t}</span>{:else}{p.t}{/if}{/each}…</span>
                </button>
              {/each}
            </article>
          {:else}
            {#if otherMatch}
              <p>当前库没有符合条件的结果，其他库有命中。</p>
              <button type="button" onclick={() => pickLibrary(otherMatch.id)}>查看 {otherMatch.title}（{otherMatch.count}）</button>
            {:else if chips.length}
              <p>当前筛选条件没有结果。<button type="button" class="link" onclick={clearFilters}>清除筛选</button></p>
            {:else}
              <p class="muted">{browsing ? "此范围没有可浏览的文档。" : "此范围没有匹配结果。试试更短的关键词或扩大搜索范围。"}</p>
            {/if}
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

{#if chooseScope}
  <LibraryLocationDialog title="添加搜索范围" libraryId={chooseScope} directoryAction="添加当前目录"
    onpick={({ library, node, path }) => { addScopes([searchScope(library, node, path)]); chooseScope = null; }}
    onclose={() => (chooseScope = null)} />
{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--paper); container-type: inline-size; }
  .bar { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .q { flex: 1 1 180px; min-width: 0; font-size: 16px; }
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
  .side { flex: none; display: flex; flex-direction: column; background: var(--panel); min-height: 0; min-width: 0; max-width: 42%; }
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
  .scope-actions { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 4px; }
  .scope-note { margin: 6px 0; color: var(--hl); font-size: 14px; }
  .scope ul { list-style: none; margin: 0; padding: 0; }
  .scope li { display: flex; align-items: center; gap: 6px; padding: 4px 0; border-bottom: 1px dashed var(--line); font-size: 14px; }
  .scope li .lab { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scope li.empty { border: none; }
  .x { font-size: 16px; color: var(--muted); }
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
  .empty-state { padding: 8px 0; }
  .availability { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .err { color: var(--hl); white-space: pre-wrap; }
  .summary { font-size: 13px; margin-bottom: 8px; }
  .unsearched { font-size: 13px; margin: 0 0 8px; overflow-wrap: anywhere; }
  .citation-actions { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-bottom: 4px; font-size: 13px; }
  .citation-actions label { display: inline-flex; gap: 4px; align-items: center; }
  article { border-bottom: 1px solid var(--line); padding: 10px 0 12px; }
  h2 { font-size: 18px; margin: 0 0 6px; font-weight: 600; display: flex; gap: 4px 10px; align-items: baseline; flex-wrap: wrap; }
  .id { font-size: 13px; font-weight: normal; }
  .snippet {
    display: flex; gap: 12px; width: 100%; text-align: left; border: none; background: none;
    padding: 4px 6px; line-height: 1.8; font-size: 16px; border-radius: 4px;
  }
  .snippet:hover { background: var(--accent-soft); }
  .where { flex: none; width: 110px; font-size: 12px; font-family: ui-monospace, Consolas, monospace; padding-top: 4px; }
  .pager { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; justify-content: center; padding: 16px; }
  .jump { width: 56px; text-align: center; }
  @container (width < 700px) {
    .main { flex-direction: column; }
    .side { width: 100% !important; max-width: none; max-height: min(30vh, 220px); min-height: 100px; border-bottom: 1px solid var(--line); }
    .split { display: none; }
    .results { min-height: 0; }
    .libs, .chips { padding-inline: 12px; }
    .hits { padding-inline: 12px; }
    .snippet { flex-direction: column; gap: 0; padding-inline: 0; }
    .where { width: auto; padding-top: 0; }
  }
</style>
