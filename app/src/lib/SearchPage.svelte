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
  import SearchScopePane from "./SearchScopePane.svelte";
  import LibraryLocationDialog from "./LibraryLocationDialog.svelte";
  import Icon from "./Icon.svelte";
  import { addScopesToSearch, normalizeSearchScopes, searchScope } from "./searchScope.js";
  import { clipMarkdown, searchClip } from "./clip.js";

  let { s, libraries: catalog = null, onopen, onstate = null, ontitle = null, onpopout = null, onai = null, onclip = null, onsearchresult = null, onsetup = null } = $props();
  const pageId = $props.id();
  const filterPanelId = `${pageId}-filters`;
  const NO_INDEX = "还没有可搜索的知识库：请先为知识库建立索引";
  const PAGE_SIZE = 20;
  const EXACT_HELP = "勾选后按连续短语匹配，忽略标点。未勾选时使用宽松匹配，关键词可出现在文档的不同位置。";

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
  let chooseScope = $state(false);
  let unavailable = $state(null);
  let windowCatalog = $state(null);
  let picked = $state([]);
  let selectionHeight = $state(0);
  let sharing = $state(false);
  let shareMessage = $state("");
  let ran = $state({ query: "", phrase: false }); // what the shown results were searched for
  let seq = 0;
  let normalizedScopes = $state.raw(null);

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
    try {
      if (s.scopes !== normalizedScopes) {
        const scopes = await normalizeSearchScopes(s.scopes, api.noteChain);
        if (my !== seq) return;
        if (scopes.length !== s.scopes.length) s.scopes = scopes;
        normalizedScopes = s.scopes;
      }
      const r = await api.search({
        query,
        phrase,
        lib: library,
        filters: filters.length ? filters : null,
        withins: s.scopes.length ? $state.snapshot(s.scopes) : null,
        page,
        limit: PAGE_SIZE,
        merge: library == null,
      });
      if (my !== seq) return;
      result = r;
      if (library != null && library !== r.lib) {
        s.libraryPinned = false;
        s.filters = {};
      } else if (library != null && filters.length) {
        s.libraryPinned = true;
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
    if (s.phrase !== phrase || ran.query !== s.query.trim()) clearPicked();
    s.phrase = phrase;
    onstate?.();
    if (s.query.trim()) run(1);
  }

  function changed() {
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
    s.lib = id;
    s.libraryPinned = id != null;
    s.filters = {};
    changed();
  }

  // ---------------------------------------------------------------- range
  const shownScopes = $derived(normalizedScopes ?? s.scopes);
  const libraries = $derived.by(() => {
    const counts = new Map((!error ? result?.libraries ?? [] : []).map((l) => [l.id, l.count]));
    return (catalog ?? windowCatalog ?? result?.libraries ?? [])
      .map(({ id, title, index_ready }) => ({ id, title, index_ready, count: counts.get(id) }))
      .sort((a, b) => Number(b.count > 0) - Number(a.count > 0));
  });
  const libraryTitles = $derived(new Map(libraries.map(({ id, title }) => [id, title])));
  const scopedLibraries = $derived(new Set(shownScopes.filter((scope) => !scope.whole).map((scope) => scope.lib)));

  function addScopes(items, followLibrary = false) {
    if (addScopesToSearch(s, items, followLibrary)) changed();
    else onstate?.();
  }
  function removeScope(scope) {
    if (s.scopes.includes(scope)) updateScopes(s.scopes.filter((item) => item !== scope));
  }
  function updateScopes(scopes) {
    s.scopes = scopes;
    changed();
  }

  // ---------------------------------------------------------------- tabs
  let filtersOpen = $state(false);
  const switchingLibrary = $derived(busy && result && result.lib !== s.lib);
  const tabs = $derived.by(() => {
    const list = [];
    for (const [root, label] of DIMENSIONS) {
      const f = switchingLibrary ? null : result?.facets.find((x) => x.root === root);
      const n = selected(root).length;
      if (!f?.values.length && !n) continue;
      list.push({ id: root, label, n });
    }
    list.push({ id: "scope", label: "范围", n: shownScopes.length });
    return list;
  });
  const activeTab = $derived(tabs.some((t) => t.id === s.tab) ? s.tab : tabs[0].id);
  const facet = $derived(switchingLibrary ? null : result?.facets.find((f) => f.root === activeTab));

  function openFilter(id) {
    filtersOpen = !filtersOpen || id !== activeTab;
    s.tab = id;
    onstate?.();
  }

  function filterKeys(event, index) {
    const { key, currentTarget } = event;
    let next;
    if (key === "ArrowUp") next = (index + tabs.length - 1) % tabs.length;
    else if (key === "ArrowDown") next = (index + 1) % tabs.length;
    else if (key === "Home") next = 0;
    else if (key === "End") next = tabs.length - 1;
    else return;
    event.preventDefault();
    s.tab = tabs[next].id;
    onstate?.();
    currentTarget.parentElement.children[next]?.focus();
  }

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
  const otherMatch = $derived(result?.lib && libraries.find((library) => library.id !== result.lib && library.count > 0));
  const firstOnPage = $derived(result?.hits.length ? (result.page - 1) * PAGE_SIZE + 1 : 0);
  const allMatches = $derived(result?.libraries.reduce((sum, library) => sum + library.count, 0) ?? 0);

  function goToPage(event) {
    event.preventDefault();
    const field = event.currentTarget.elements.namedItem("page");
    const value = Number.parseInt(field.value, 10);
    const page = Number.isFinite(value) ? Math.min(Math.max(value, 1), result.pages) : result.page;
    field.value = String(page);
    if (page !== result.page) run(page);
  }

  const snippetKey = (hit, snippet) => JSON.stringify([hit.lib, hit.path, snippet.lb, snippet.pos]);
  const pickedKeys = $derived(new Set(picked.map((item) => item.key)));

  function clearPicked() {
    picked = [];
    shareMessage = "";
  }

  function pickSnippet(hit, snippet, checked) {
    const key = snippetKey(hit, snippet);
    picked = checked ? [...picked, { key, clip: searchClip(hit, snippet) }] : picked.filter((item) => item.key !== key);
    shareMessage = "";
  }

  async function shareClips(action) {
    if (!picked.length || sharing) return;
    const clips = $state.snapshot(picked).map((item) => item.clip);
    sharing = true;
    shareMessage = "";
    try {
      if (action === "note") {
        await onclip(clips);
        return;
      }
      if (action === "ai") {
        await onai({ quotes: clips.map(({ text, vol, lb, position }) => ({
          text,
          source: [vol.libraryTitle, vol.title, vol.label, lb, `第${position}字`].filter(Boolean).join(" · "),
        })) });
        shareMessage = `已加入 AI 会话 ${clips.length} 条摘录`;
        return;
      }
      let path;
      if (action === "export") {
        path = await saveDialog({ defaultPath: "搜索摘录.md", filters: [{ name: "Markdown", extensions: ["md"] }] });
        if (!path) return;
      }
      const markdown = (await Promise.all(clips.map((clip) => clipMarkdown(clip)))).join("\n\n") + "\n";
      if (path) await api.saveBytes(path, new TextEncoder().encode(markdown));
      else await navigator.clipboard.writeText(markdown);
      shareMessage = `${path ? "已导出" : "已复制"} ${clips.length} 条摘录`;
    } catch (e) {
      shareMessage = String(e);
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
    // Older search tabs used "scope" as their default even when no scope was set.
    if (s.tab === "scope" && !s.scopes.length) s.tab = null;
    ontitle?.(title());
    if (s.focusOnOpen || !s.ran && !s.autorun) input?.focus();
    if (s.ran || s.autorun) {
      s.autorun = false;
      run(s.page || 1);
    }
    function refreshWindowCatalog() {
      api.status().then((status) => (windowCatalog = status.libraries)).catch(() => {});
    }
    if (catalog === null) {
      refreshWindowCatalog();
      window.addEventListener("focus", refreshWindowCatalog);
    }
    const off = api.onIndexDone((event) => {
      if (catalog === null) refreshWindowCatalog();
      if (event.ok && (unavailable || error === NO_INDEX || result?.index_unavailable && result.lib === event.lib)) run(1);
    });
    return () => {
      window.removeEventListener("focus", refreshWindowCatalog);
      off.then((unlisten) => unlisten());
    };
  });
</script>

<div class="page" class:docked={!!onpopout}>
  {#if libraries.length}
    <div class="libs" role="group" aria-label="选择要搜索的知识库">
      <button type="button" aria-pressed={!s.libraryPinned} class:on={!s.libraryPinned} onclick={() => pickLibrary(null)}>
        全部{#if result && !error}<span class="n">{allMatches}</span>{/if}
      </button>
      {#each libraries as l (l.id)}
        <button type="button" class:on={s.libraryPinned && l.id === s.lib} aria-pressed={s.libraryPinned && l.id === s.lib} onclick={() => pickLibrary(l.id)}
          title={scopedLibraries.has(l.id) ? "已限定搜索范围" : l.index_ready === false ? "全文索引暂不可用" : undefined}>
          {l.title}{scopedLibraries.has(l.id) ? "*" : ""}{#if l.count != null}<span class="n">{l.count}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  <form class="bar" onsubmit={submit}>
    <div class="query-tools">
      <input type="search" class="q" bind:this={input} bind:value={s.query} oninput={() => onstate?.()}
        placeholder="输入关键词" aria-label="全文搜索" title="留空可浏览全部文档" />
      <button class="primary submit" disabled={busy}>{busy ? "搜索中…" : "搜索"}</button>
      <label class="exact" title={EXACT_HELP}>
        <input type="checkbox" checked={s.phrase} title={EXACT_HELP} onchange={(event) => setMode(event.currentTarget.checked)} />
        精确
      </label>
    </div>
    {#if onpopout}
      <button type="button" class="view-action" onclick={onpopout} title="在新窗口打开这个搜索">
        <Icon name="open-window" size={15} /> 新窗口
      </button>
    {/if}
  </form>

  <div class="main" class:filters-open={filtersOpen} style="--search-side-width: {sideWidth}px">
    <div class="filter-rail">
      <button type="button" class="filter-toggle" class:expanded={filtersOpen} aria-expanded={filtersOpen} aria-controls={filterPanelId}
        aria-label={filtersOpen ? "收起筛选面板" : "展开筛选面板"} title={filtersOpen ? "收起筛选面板" : "展开筛选面板"}
        onclick={() => (filtersOpen = !filtersOpen)}>
        <Icon name="chevron-right" size={14} />
      </button>
      <div class="tabs" role="tablist" aria-label="筛选类别" aria-orientation="vertical">
        {#each tabs as t, i (t.id)}
          <button type="button" role="tab" id={`${filterPanelId}-${i}`} aria-selected={filtersOpen && activeTab === t.id} aria-controls={filterPanelId}
            tabindex={activeTab === t.id ? 0 : -1} class:on={filtersOpen && activeTab === t.id} onclick={() => openFilter(t.id)}
            onkeydown={(e) => filterKeys(e, i)}
            data-scope-drop={t.id === "scope" ? "" : null} onscopedrop={(e) => addScopes(e.detail, true)}>
            <span class="tab-label">{t.label}</span>{#if t.n}<span class="dot">{t.n}</span>{/if}
          </button>
        {/each}
      </div>
    </div>
    <div class="side" role="tabpanel" id={filterPanelId} aria-labelledby={`${filterPanelId}-${tabs.findIndex((t) => t.id === activeTab)}`} hidden={!filtersOpen}>
      {#if activeTab === "scope"}
        <SearchScopePane scopes={shownScopes} {libraryTitles} onchoose={() => (chooseScope = true)}
          onadd={(items) => addScopes(items, true)} onremove={removeScope} onclear={() => updateScopes([])} />
      {:else if facet}
        {#key facet.root}
          <FacetPane {facet} selected={selected(facet.root)} ontoggle={(n) => toggle(facet.root, n)} onclear={() => clearDim(facet.root)} />
        {/key}
      {:else}
        <p class="muted hint">点「搜索」后，这里列出可以用来过滤的{DIMENSIONS.map((d) => d[1]).join("、")}。</p>
      {/if}
    </div>
    {#if filtersOpen}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="split" role="separator" aria-orientation="vertical" aria-label="调整筛选栏宽度" aria-valuenow={sideWidth} tabindex="0" onpointerdown={dragSide} onkeydown={resizeSide}
        ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>
    {/if}

    <section class="results" aria-busy={busy}>
      {#if chips.length}
        <div class="chips">
          {#each chips as c (c.root + c.name)}
            <span class="chip">{labelOf(c.root)}：{c.name}
              <button class="link" onclick={() => toggle(c.root, c.name)} aria-label="取消">×</button></span>
          {/each}
          <button class="link" onclick={clearFilters}>清除过滤</button>
        </div>
      {/if}
      <div class="hits" bind:this={hitsEl} style:--selection-space={picked.length ? `${selectionHeight + 10}px` : "0px"}>
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
          <p class="muted" role="status">正在载入搜索结果…</p>
        {:else if !result}
          <p class="muted intro">
            输入关键词后按 Enter。留空搜索可以浏览当前范围内的文档。
          </p>
        {:else if result.index_unavailable}
          <p>「{libraryTitles.get(result.lib)}」的全文索引暂不可用。</p>
          {#if onsetup}<button type="button" onclick={() => onsetup({ type: "index", id: result.lib })}>查看索引…</button>{/if}
        {:else}
          {#if result.unavailable_libraries.length}
            <p class="muted unsearched">未参与全文搜索：{result.unavailable_libraries.join("、")}</p>
          {/if}
          {#each result.hits as h}
            <article>
              <h2>
                <button class="link" onclick={() => open(h, null)}>{h.title} {h.book_id ? h.subtitle : ""}</button>
                {#if result.lib == null}
                  <button type="button" class="link source" onclick={() => pickLibrary(h.lib)} title="只看此知识库">{h.library_title}</button>
                {/if}
                <span class="muted id">{[h.book_id, h.book_id ? "" : h.subtitle, !browsing && h.occurrences ? `${h.occurrences} 处` : ""].filter(Boolean).join(" · ")}</span>
              </h2>
              {#each h.snippets as sn}
                {@const checked = pickedKeys.has(snippetKey(h, sn))}
                <div class="snippet-row" class:checked>
                  <label class="snippet-select">
                    <input type="checkbox" class="snippet-check" {checked}
                      aria-label={`选择《${h.title}》${sn.lb} 第${sn.pos}字的摘录`}
                      onchange={(event) => pickSnippet(h, sn, event.currentTarget.checked)} />
                  </label>
                  <button class="snippet" onclick={() => open(h, sn.lb)}>
                    <span class="where muted">{sn.lb} 第{sn.pos}字</span>
                    <span class="excerpt">…{#each sn.parts as p}{#if p.hl}<span class="hl">{p.t}</span>{:else}{p.t}{/if}{/each}…</span>
                  </button>
                </div>
              {/each}
            </article>
          {:else}
            {#if chips.length}
              <p>当前筛选条件没有结果。<button type="button" class="link" onclick={clearFilters}>清除筛选</button></p>
            {:else if otherMatch}
              <p>当前库没有符合条件的结果，其他库有命中。</p>
              <button type="button" onclick={() => pickLibrary(otherMatch.id)}>查看 {otherMatch.title}（{otherMatch.count}）</button>
            {:else}
              <p class="muted">{browsing ? "此范围没有可浏览的文档。" : "此范围没有匹配结果。试试更短的关键词或扩大搜索范围。"}</p>
            {/if}
          {/each}
          <footer class="pager">
            <span class="page-range" aria-live="polite">
              {#if firstOnPage}第 {firstOnPage}–{firstOnPage + result.hits.length - 1} 条 · {/if}
              {#if result.kind === "cbeta"}
                共 <strong>{result.total_books}</strong> 部、<strong>{result.total_volumes}</strong> 卷
              {:else}
                共 <strong>{result.total_volumes}</strong> {browsing ? "篇文档" : "篇匹配文档"}
              {/if}
            </span>
            {#if result.pages > 1}
              <form class="page-controls" aria-label="搜索结果分页" onsubmit={goToPage}>
                <button type="button" aria-label="首页" title="首页" disabled={result.page <= 1 || busy} onclick={() => run(1)}>
                  <Icon name="page-first" size={16} />
                </button>
                <button type="button" aria-label="上一页" title="上一页" disabled={result.page <= 1 || busy} onclick={() => run(result.page - 1)}>
                  <Icon name="arrow-left" size={16} />
                </button>
                <label class="page-number">第
                  {#key result.page}
                    <input type="text" inputmode="numeric" name="page" value={result.page} disabled={busy} aria-label="页码"
                      title="输入页码，按 Enter 跳转" onfocus={(event) => event.currentTarget.select()} />
                  {/key}
                  / {result.pages} 页
                </label>
                <button type="button" aria-label="下一页" title="下一页" disabled={result.page >= result.pages || busy} onclick={() => run(result.page + 1)}>
                  <Icon name="arrow-right" size={16} />
                </button>
                <button type="button" aria-label="末页" title="末页" disabled={result.page >= result.pages || busy} onclick={() => run(result.pages)}>
                  <Icon name="page-last" size={16} />
                </button>
              </form>
            {/if}
          </footer>
        {/if}
      </div>
      {#if picked.length}
        <div class="selection-bar" role="group" aria-label="已选摘录" bind:offsetHeight={selectionHeight}>
          <span class="selection-count" aria-live="polite">已选择 {picked.length} 条</span>
          <button type="button" onclick={() => shareClips("copy")} disabled={sharing}>复制摘录</button>
          {#if onclip}<button type="button" onclick={() => shareClips("note")} disabled={sharing}>加入笔记…</button>{/if}
          {#if onai && settings.aiOn}<button type="button" onclick={() => shareClips("ai")} disabled={sharing}>加入 AI 会话</button>{/if}
          <button type="button" onclick={() => shareClips("export")} disabled={sharing}>导出 Markdown…</button>
          <button type="button" class="link" onclick={clearPicked} disabled={sharing}>清空选择</button>
          {#if shareMessage}<span class="muted share-message" role="status">{shareMessage}</span>{/if}
        </div>
      {/if}
    </section>
  </div>
</div>

{#if chooseScope}
  <LibraryLocationDialog title="添加搜索范围" directoryAction="添加当前目录" rootAction="添加整个知识库"
    onpick={({ library, node, path, depth }) => { addScopes([searchScope(library, node, path, depth)]); chooseScope = false; }}
    onclose={() => (chooseScope = false)} />
{/if}

<style>
  .page { --rail-width: 34px; --result-gutter: 20px; display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--paper); container-type: inline-size; }
  .page.docked { border-block-start: 1px solid var(--line); }
  .libs, .bar, .filter-rail, .side, .split { background: var(--panel); }
  .libs, .bar { flex: none; border-bottom: 1px solid var(--line); }
  .libs { display: flex; gap: 2px; padding-block-start: 4px; overflow-x: auto; scrollbar-width: thin; }
  .libs button {
    flex: none; white-space: nowrap; padding: 6px 10px; font-size: 13px; color: var(--muted);
    border: none; border-bottom: 2px solid transparent; border-radius: 5px 5px 0 0; background: none;
  }
  .libs button:hover { color: var(--ink); background: var(--accent-soft); }
  .libs button.on { color: var(--accent); font-weight: 600; border-bottom-color: var(--accent); }
  .libs .n { color: var(--muted); margin-inline-start: 6px; font-size: 11px; font-weight: 400; font-variant-numeric: tabular-nums; }
  .bar { display: flex; flex-wrap: wrap; gap: 8px 12px; align-items: center; min-width: 0; padding: 10px var(--rail-width); }
  .query-tools { display: flex; flex: 1 1 260px; align-items: center; gap: 8px; min-width: 0; }
  .q { flex: 1 1 0; min-width: 0; height: 34px; font-size: 16px; }
  .submit { flex: none; min-width: 67px; padding: 4px 8px; font-size: 14px; }
  .exact { display: inline-flex; flex: none; align-items: center; gap: 5px; min-height: 30px; padding-inline: 4px; font-size: 13px; cursor: pointer; }
  .exact input { margin: 0; accent-color: var(--accent); cursor: pointer; }
  .view-action { display: inline-flex; flex: none; align-items: center; gap: 5px; margin-inline-start: auto; padding: 4px 8px; border-color: transparent; background: transparent; color: var(--accent); font-size: 13px; }
  .view-action:hover { background: var(--accent-soft); }
  .main {
    position: relative; flex: 1; display: grid; min-height: 0;
    grid-template: "rail results" minmax(0, 1fr) / var(--rail-width) minmax(0, 1fr);
  }
  .main.filters-open {
    grid-template: "rail filters divider results" minmax(0, 1fr) /
      var(--rail-width) min(var(--search-side-width), 42%) 5px minmax(0, 1fr);
  }
  .filter-rail {
    grid-area: rail; display: flex; flex-direction: column; align-items: stretch; min-height: 0;
    border-inline-end: 1px solid var(--line);
  }
  .filter-toggle {
    display: grid; place-items: center; flex: none; margin: 5px; height: 28px; padding: 0;
    color: var(--muted); border-color: transparent; background: none;
  }
  .filter-toggle.expanded > :global(svg) { transform: rotate(180deg); }
  .filter-toggle:hover, .tabs button:hover { background: var(--accent-soft); color: var(--accent); }
  .side { grid-area: filters; display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .side[hidden] { display: none; }
  .side > :global(*) { flex: 1; min-height: 0; }
  .split { grid-area: divider; cursor: col-resize; border-inline-start: 1px solid var(--line); touch-action: none; }
  .split:hover, .split:focus-visible { background: var(--accent-soft); outline: none; }
  .tabs { display: flex; flex-direction: column; gap: 2px; padding: 3px 0; overflow: auto; scrollbar-width: thin; }
  .tabs button {
    position: relative; flex: none; display: grid; place-items: center; min-height: 46px; padding: 4px 0;
    border: none; border-inline-start: 2px solid transparent; border-radius: 0;
    background: none; color: var(--muted); font-size: 13px;
  }
  .tabs button.on { border-inline-start-color: var(--accent); color: var(--accent); background: var(--accent-soft); font-weight: 600; }
  .tabs button:focus-visible, .tabs button:global(.scope-over) { outline: 1px solid var(--accent); outline-offset: -1px; }
  .tabs button:global(.scope-over) { background: var(--accent-soft); }
  .tab-label { writing-mode: vertical-rl; letter-spacing: .12em; }
  .dot { position: absolute; inset-block-start: 2px; inset-inline-end: 2px; font-size: 9px; line-height: 1.3; background: var(--accent); color: var(--paper); border-radius: 8px; padding: 0 3px; }
  .hint { padding: 12px; }
  .results { position: relative; grid-area: results; min-width: 0; min-height: 0; display: flex; flex-direction: column; background: var(--paper); }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; padding: 8px var(--result-gutter) 0; font-size: 13px; }
  .chip { background: var(--accent-soft); border-radius: 12px; padding: 1px 4px 1px 10px; }
  .chip .link { padding: 0 4px; }
  .hits { flex: 1; min-height: 0; overflow: auto; padding: 4px var(--result-gutter) calc(14px + var(--selection-space)); scroll-padding-block-end: var(--selection-space); }
  .intro, .err { padding-top: 8px; }
  .empty-state { padding: 8px 0; }
  .availability { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .err { color: var(--hl); white-space: pre-wrap; }
  .unsearched { font-size: 13px; margin: 8px 0; overflow-wrap: anywhere; }
  .selection-bar {
    position: absolute; z-index: 1; inset-inline: var(--result-gutter); inset-block-end: 10px;
    display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; padding: 8px 10px;
    border: 1px solid var(--line); border-radius: 8px; background: var(--panel);
    box-shadow: 0 3px 12px #0002; font-size: 13px;
  }
  .selection-bar button { padding: 3px 8px; }
  .selection-count { white-space: nowrap; font-weight: 600; color: var(--accent); }
  .share-message { font-size: 12px; }
  article { border-bottom: 1px solid var(--line); padding: 10px 0 12px; }
  h2 { font-size: 18px; margin: 0 0 6px; font-weight: 600; display: flex; gap: 4px 10px; align-items: baseline; flex-wrap: wrap; }
  .source { padding: 1px 6px; border-radius: 4px; background: var(--accent-soft); font-size: 12px; font-weight: normal; }
  .source:hover { text-decoration: underline; }
  .id { font-size: 13px; font-weight: normal; }
  .snippet-row {
    --snippet-line: 1.75rem; --snippet-padding: 4px;
    display: grid; grid-template-columns: 24px minmax(0, 1fr); align-items: start; border-radius: 4px;
  }
  .snippet-select { display: grid; place-items: center; height: var(--snippet-line); margin-block-start: var(--snippet-padding); cursor: pointer; }
  .snippet-check { margin: 0; accent-color: var(--accent); cursor: pointer; }
  @media (hover: hover) {
    .snippet-check { opacity: 0; }
    .snippet-row:hover .snippet-check, .snippet-row:focus-within .snippet-check, .snippet-row.checked .snippet-check { opacity: 1; }
  }
  .snippet-row:hover, .snippet-row.checked { background: var(--accent-soft); }
  .snippet {
    display: grid; grid-template-columns: 110px minmax(0, 1fr); align-items: start; gap: 12px;
    text-align: left; border: none; background: none; border-radius: 4px;
    padding: var(--snippet-padding) 6px; line-height: var(--snippet-line); font-size: 16px;
  }
  .where { font-size: 12px; font-family: ui-monospace, Consolas, monospace; }
  .excerpt { overflow-wrap: anywhere; }
  .pager { display: flex; align-items: center; justify-content: space-between; gap: 6px 12px; flex-wrap: wrap; margin-top: 8px; padding: 8px 0 0; font-size: 13px; }
  .page-range { color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .page-controls { display: flex; align-items: center; gap: 4px; margin-inline-start: auto; }
  .page-controls button { display: grid; place-items: center; padding: 0; width: 30px; height: 28px; color: var(--accent); }
  .page-controls button:disabled, .selection-bar button:disabled { opacity: .48; cursor: default; }
  .page-range strong { color: var(--accent); }
  .page-number { display: flex; align-items: center; gap: 5px; padding: 0 5px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .page-number input { width: 5ch; height: 28px; padding: 2px; text-align: center; }
  @container (width < 700px) {
    .main { --result-gutter: 12px; }
    .snippet { grid-template-columns: minmax(0, 1fr); gap: 2px; padding-inline: 0; }
    .excerpt { grid-row: 1; }
    .where { grid-row: 2; line-height: 1.4; }
  }
  @container (width < 620px) {
    .main.filters-open { grid-template: "rail results" minmax(0, 1fr) / var(--rail-width) minmax(0, 1fr); }
    .side {
      position: absolute; z-index: 2; grid-area: auto; inset-block: 0; inset-inline-start: var(--rail-width);
      width: min(var(--search-side-width), calc(100% - var(--rail-width) - 24px));
      border-inline-end: 1px solid var(--line); box-shadow: 5px 0 14px #0002;
    }
    .split { display: none; }
  }
  @container (width < 420px) {
    .view-action { margin-inline-start: 0; }
    .page-controls { margin-inline-start: 0; }
  }
  @container (width < 360px) {
    .query-tools { flex-wrap: wrap; }
    .q { flex-basis: 100%; }
  }
</style>
