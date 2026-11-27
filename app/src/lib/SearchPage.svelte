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
  import { searchCitation } from "./clip.js";

  let { s, onopen, onstate = null, ontitle = null, onpopout = null, onai = null, onsearchresult = null, onsetup = null } = $props();
  const NO_INDEX = "还没有可搜索的知识库：请先为知识库建立索引";
  const PAGE_SIZE = 20;

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
  let selecting = $state(false);
  let picked = $state([]);
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
    s.lib = id;
    s.libraryPinned = id != null;
    s.filters = {};
    changed(keepPicked);
  }

  // ---------------------------------------------------------------- range
  const shownScopes = $derived(normalizedScopes ?? s.scopes);
  const libraries = $derived(result ? [...result.libraries].sort((a, b) => Number(b.count > 0) - Number(a.count > 0)) : []);
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
  const switchingLibrary = $derived(busy && result && result.lib !== s.lib);
  const tabs = $derived.by(() => {
    const list = [{ id: "scope", label: "范围", n: shownScopes.length }];
    for (const [root, label] of DIMENSIONS) {
      const f = switchingLibrary ? null : result?.facets.find((x) => x.root === root);
      const n = selected(root).length;
      if (!f?.values.length && !n) continue;
      list.push({ id: root, label, n });
    }
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
  const otherMatch = $derived(result?.lib && libraries.find((library) => library.id !== result.lib && library.count > 0));
  const showResultTools = $derived(result && !error && !switchingLibrary && !result.index_unavailable);
  const firstOnPage = $derived(result?.hits.length ? (result.page - 1) * PAGE_SIZE + 1 : 0);

  function goToPage(event) {
    event.preventDefault();
    const field = event.currentTarget.elements.namedItem("page");
    const value = Number.parseInt(field.value, 10);
    const page = Number.isFinite(value) ? Math.min(Math.max(value, 1), result.pages) : result.page;
    field.value = String(page);
    if (page !== result.page) run(page);
  }

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
      picked = [...picked, ...hits.filter((hit) => !pickedKeys.has(hitKey(hit))).map((hit) => ({ hit, libraryTitle: hit.library_title }))];
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
      if (event.ok && (unavailable || error === NO_INDEX || result?.index_unavailable && result.lib === event.lib)) run(1);
    });
    return () => off.then((unlisten) => unlisten());
  });
</script>

<div class="page">
  <form class="bar" onsubmit={submit}>
    <div class="query-tools">
      <div class="query-field">
        <span class="query-icon"><Icon name="search" size={16} /></span>
        <input type="search" class="q" bind:this={input} bind:value={s.query} oninput={() => onstate?.()}
          placeholder="搜索全文" aria-label="全文搜索" />
      </div>
      <select class="mode" aria-label="匹配方式" value={s.phrase ? "phrase" : "loose"}
        title={s.phrase ? "整句连续出现（忽略标点）" : "用空格分隔关键词，各组可以出现在文档的不同位置"}
        onchange={(event) => setMode(event.currentTarget.value === "phrase")}>
        <option value="loose">宽松匹配</option>
        <option value="phrase">精确短语</option>
      </select>
      <button class="primary submit" disabled={busy}>{busy ? "搜索中…" : "搜索"}</button>
    </div>
    <div class="view-tools">
      {#if settings.aiOn && onai}
        <button type="button" class="view-action" onclick={() => onai({ toggle: true })} title="打开 / 收起右侧 AI 助手栏（F1）">
          <Icon name="sparkles" size={16} /> AI 助手
        </button>
      {/if}
      {#if onpopout}
        <button type="button" class="view-action" onclick={onpopout} title="在新窗口打开这个搜索">
          <Icon name="open-window" size={15} /> 新窗口
        </button>
      {/if}
    </div>
  </form>

  <div class="main">
    <aside class="side" style="width: {sideWidth}px">
      <div class="tabs" role="tablist">
        {#each tabs as t (t.id)}
          <button role="tab" aria-selected={activeTab === t.id} class:on={activeTab === t.id} onclick={() => ((s.tab = t.id), onstate?.())}
            data-scope-drop={t.id === "scope" ? "" : null} onscopedrop={(e) => addScopes(e.detail, true)}>
            {t.label}{#if t.n}<span class="dot">{t.n}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="panes">
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
    </aside>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="split" role="separator" aria-orientation="vertical" aria-label="调整筛选栏宽度" aria-valuenow={sideWidth} tabindex="0" onpointerdown={dragSide} onkeydown={resizeSide}
      ondblclick={() => (sideWidth = 300)} title="拖动调整宽度，双击还原"></div>

    <section class="results" aria-busy={busy}>
      {#if result && !error && result.libraries.length > 1}
        <div class="libs" role="group" aria-label="显示哪个知识库的结果">
          <button aria-pressed={!s.libraryPinned} class:on={!s.libraryPinned} onclick={() => pickLibrary(null)}>全部知识库</button>
          {#each libraries as l (l.id)}
            <button class:on={s.libraryPinned && l.id === s.lib} aria-pressed={s.libraryPinned && l.id === s.lib} onclick={() => pickLibrary(l.id)}
              title={scopedLibraries.has(l.id) ? "已限定搜索范围" : null}>
              {l.title}{scopedLibraries.has(l.id) ? "*" : ""} <span class="n">{l.count}</span>
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
      {#if showResultTools}
        <div class="result-head">
          <p class="summary" aria-live="polite">
            {#if result.kind === "cbeta"}
              {browsing ? "共" : "命中"} <strong>{result.total_books.toLocaleString()}</strong> 部、<strong>{result.total_volumes.toLocaleString()}</strong> 卷
            {:else}
              {browsing ? "共" : "找到"} <strong>{result.total_volumes.toLocaleString()}</strong> {browsing ? "篇文档" : "篇匹配文档"}
            {/if}
          </p>
          {#if result.hits.length || picked.length}
            <div class="citation-actions">
              {#if selecting}
                {#if result.hits.length}
                  <label><input type="checkbox" checked={allPagePicked} onchange={(event) => pickHits(result.hits, event.currentTarget.checked)} />全选本页</label>
                {/if}
                <span class="muted selection-count">已选 {picked.length} 条</span>
                <button type="button" onclick={() => shareCitations()} disabled={!picked.length || sharing}>复制引用</button>
                <button type="button" onclick={() => shareCitations(true)} disabled={!picked.length || sharing}>导出 Markdown…</button>
                <button type="button" class="link" onclick={() => { selecting = false; clearPicked(); }} disabled={sharing}>退出选择</button>
              {:else}
                <button type="button" onclick={() => (selecting = true)}>选择结果</button>
              {/if}
            </div>
          {/if}
          {#if shareMessage}<p class="muted share-message" role="status">{shareMessage}</p>{/if}
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
                {#if selecting}<input type="checkbox" checked={pickedKeys.has(hitKey(h))} aria-label={`选择 ${h.title} ${h.subtitle}`}
                  onchange={(event) => pickHits([h], event.currentTarget.checked)} />{/if}
                <button class="link" onclick={() => open(h, null)}>{h.title} {h.book_id ? h.subtitle : ""}</button>
                {#if result.lib == null}
                  <button type="button" class="link source" onclick={() => pickLibrary(h.lib)} title="只看此知识库">{h.library_title}</button>
                {/if}
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
            {#if chips.length}
              <p>当前筛选条件没有结果。<button type="button" class="link" onclick={clearFilters}>清除筛选</button></p>
            {:else if otherMatch}
              <p>当前库没有符合条件的结果，其他库有命中。</p>
              <button type="button" onclick={() => pickLibrary(otherMatch.id)}>查看 {otherMatch.title}（{otherMatch.count}）</button>
            {:else}
              <p class="muted">{browsing ? "此范围没有可浏览的文档。" : "此范围没有匹配结果。试试更短的关键词或扩大搜索范围。"}</p>
            {/if}
          {/each}
        {/if}
      </div>
      {#if showResultTools && result.pages > 1}
        <nav class="pager" aria-label="搜索结果分页">
          {#if firstOnPage}
            <span class="page-range">第 {firstOnPage.toLocaleString()}–{(firstOnPage + result.hits.length - 1).toLocaleString()} 条</span>
          {/if}
          <form class="page-controls" onsubmit={goToPage}>
            <button type="button" aria-label="首页" title="首页" disabled={result.page <= 1 || busy} onclick={() => run(1)}>«</button>
            <button type="button" aria-label="上一页" title="上一页" disabled={result.page <= 1 || busy} onclick={() => run(result.page - 1)}>
              <Icon name="arrow-left" size={15} />
            </button>
            <label class="page-number">第
              {#key result.page}
                <input type="text" inputmode="numeric" name="page" value={result.page} disabled={busy} aria-label="页码"
                  title="输入页码，按 Enter 跳转" onfocus={(event) => event.currentTarget.select()} />
              {/key}
              / {result.pages.toLocaleString()} 页
            </label>
            <button type="button" aria-label="下一页" title="下一页" disabled={result.page >= result.pages || busy} onclick={() => run(result.page + 1)}>
              <Icon name="arrow-right" size={15} />
            </button>
            <button type="button" aria-label="末页" title="末页" disabled={result.page >= result.pages || busy} onclick={() => run(result.pages)}>»</button>
          </form>
        </nav>
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
  .page { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--paper); container-type: inline-size; }
  .bar { display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: center; padding: 10px 16px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .query-tools { display: flex; align-items: stretch; gap: 7px; flex: 1 1 390px; min-width: 0; max-width: 535px; }
  .query-field { position: relative; flex: 1; min-width: 0; }
  .query-icon { position: absolute; inset-inline-start: 10px; top: 50%; transform: translateY(-50%); color: var(--muted); pointer-events: none; }
  .q { width: 100%; height: 34px; padding-inline-start: 33px; font-size: 15px; }
  .mode { flex: none; max-width: 40%; font-size: 13px; }
  .submit { flex: none; min-width: 67px; padding: 4px 8px; font-size: 14px; }
  .view-tools { display: flex; flex-wrap: wrap; gap: 4px; margin-inline-start: auto; }
  .view-tools:empty { display: none; }
  .view-action { display: inline-flex; align-items: center; gap: 5px; padding: 4px 8px; border-color: transparent; background: transparent; color: var(--accent); font-size: 13px; }
  .view-action:hover { background: var(--accent-soft); }
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
  .tabs button:global(.scope-over) { background: var(--accent-soft); outline: 1px solid var(--accent); }
  .dot { margin-left: 3px; font-size: 11px; background: var(--accent); color: var(--paper); border-radius: 8px; padding: 0 5px; }
  .panes { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .panes > :global(*) { flex: 1; min-height: 0; }
  .hint { padding: 12px; }
  .results { --result-gutter: 20px; flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .libs { display: flex; gap: 6px; flex-wrap: wrap; padding: 10px var(--result-gutter) 0; }
  .libs button { border-radius: 14px; font-size: 13px; padding: 2px 12px; }
  .libs button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .libs .n { opacity: 0.75; margin-left: 4px; }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; padding: 8px var(--result-gutter) 0; font-size: 13px; }
  .chip { background: var(--accent-soft); border-radius: 12px; padding: 1px 4px 1px 10px; }
  .chip .link { padding: 0 4px; }
  .result-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 6px 12px; padding: 8px var(--result-gutter); min-height: 45px; border-bottom: 1px solid var(--line); }
  .summary { margin: 0; color: var(--muted); font-size: 13px; }
  .summary strong { color: var(--accent); font-size: 16px; font-variant-numeric: tabular-nums; }
  .hits { flex: 1; min-height: 0; overflow: auto; padding: 4px var(--result-gutter) 14px; }
  .intro, .err { padding-top: 8px; }
  .empty-state { padding: 8px 0; }
  .availability { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .err { color: var(--hl); white-space: pre-wrap; }
  .unsearched { font-size: 13px; margin: 8px 0; overflow-wrap: anywhere; }
  .citation-actions { display: flex; flex-wrap: wrap; gap: 6px 8px; align-items: center; font-size: 13px; }
  .citation-actions button { padding: 3px 8px; }
  .citation-actions label { display: inline-flex; gap: 4px; align-items: center; white-space: nowrap; }
  .selection-count { white-space: nowrap; }
  .share-message { flex-basis: 100%; margin: 0; font-size: 13px; }
  article { border-bottom: 1px solid var(--line); padding: 10px 0 12px; }
  h2 { font-size: 18px; margin: 0 0 6px; font-weight: 600; display: flex; gap: 4px 10px; align-items: baseline; flex-wrap: wrap; }
  .source { padding: 1px 6px; border-radius: 4px; background: var(--accent-soft); font-size: 12px; font-weight: normal; }
  .source:hover { text-decoration: underline; }
  .id { font-size: 13px; font-weight: normal; }
  .snippet {
    display: flex; gap: 12px; width: 100%; text-align: left; border: none; background: none;
    padding: 4px 6px; line-height: 1.8; font-size: 16px; border-radius: 4px;
  }
  .snippet:hover { background: var(--accent-soft); }
  .where { flex: none; width: 110px; font-size: 12px; font-family: ui-monospace, Consolas, monospace; padding-top: 4px; }
  .pager { display: flex; align-items: center; justify-content: space-between; gap: 6px 12px; flex-wrap: wrap; padding: 7px var(--result-gutter); border-top: 1px solid var(--line); background: var(--panel); font-size: 13px; }
  .page-range { color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .page-controls { display: flex; align-items: center; gap: 4px; margin-inline-start: auto; }
  .page-controls button { display: grid; place-items: center; padding: 0; width: 30px; height: 28px; color: var(--accent); font-size: 17px; }
  .page-controls button:disabled, .citation-actions button:disabled { opacity: .48; cursor: default; }
  .page-number { display: flex; align-items: center; gap: 5px; padding: 0 5px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .page-number input { width: 5ch; height: 28px; padding: 2px; text-align: center; }
  @container (width < 700px) {
    .main { flex-direction: column; }
    .side { width: 100% !important; max-width: none; max-height: min(30vh, 220px); min-height: 100px; border-bottom: 1px solid var(--line); }
    .split { display: none; }
    .results { --result-gutter: 12px; }
    .snippet { flex-direction: column; gap: 0; padding-inline: 0; }
    .where { width: auto; padding-top: 0; }
  }
  @container (width < 420px) {
    .bar { padding-inline: 10px; }
    .view-tools { margin-inline-start: 0; }
    .page-controls { margin-inline-start: 0; }
  }
</style>
