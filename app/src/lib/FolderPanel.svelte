<script>
  // Directory card/list view inside a workspace tab. Shows the direct children
  // of a library root or a traditional folder node, with breadcrumbs, quick
  // filtering, and card/list layout toggle.
  import * as api from "./api.js";
  import Icon from "./Icon.svelte";

  const VIEW_STORAGE_KEY = "sl.folderViewMode";

  let { p, getLibrary, onnavigate, onopen, onlocate = null, ontitle = null, onrules = null } = $props();

  let items = $state([]); // { node, childKey }[]
  let excluded = $state([]);
  let crumbs = $state([]); // { title, rel, key }[]
  let seen = $state({ books: new Set(), paths: new Set() });
  let loading = $state(false);
  let error = $state("");
  let filter = $state("");
  let viewMode = $state(readViewMode());

  function readViewMode() {
    try {
      return localStorage.getItem(VIEW_STORAGE_KEY) === "list" ? "list" : "grid";
    } catch {
      return "grid";
    }
  }

  function setViewMode(mode) {
    viewMode = mode;
    try {
      localStorage.setItem(VIEW_STORAGE_KEY, mode);
    } catch {}
  }

  const isDoc = (n) => Boolean(n.book_id || n.path);
  const isUnsupported = (n) => !n.book_id && !n.path && Boolean(n.href);
  const isDir = (n) => !isDoc(n) && !isUnsupported(n);
  const wasOpened = (n) => Boolean((n.book_id && seen.books.has(n.book_id)) || (n.path && seen.paths.has(n.path)));

  function itemIcon(n) {
    if (n.mind) return "mindmap";
    if (n.board) return "whiteboard";
    if (isDir(n) || n.children?.length > 0 || n.lazy) return "folder";
    return "file";
  }

  function itemMeta(n) {
    if (isUnsupported(n)) return "未接入";
    if (isDir(n)) return n.children?.length > 0 ? `${n.children.length} 项` : "文件夹";
    if (n.mind) return "脑图";
    if (n.board) return "白板";
    if (n.children?.length > 0 || n.lazy) return n.children?.length > 0 ? `笔记 · ${n.children.length} 子项` : "笔记";
    if (n.book_id) return n.book_id;
    const rel = n.rel || n.path || "";
    const dot = rel.lastIndexOf(".");
    if (dot > rel.lastIndexOf("/") && dot >= 0) {
      const ext = rel.slice(dot + 1).toUpperCase();
      if (ext === "MD" || ext === "MARKDOWN") return "文档";
      if (ext && ext.length <= 6) return ext;
    }
    return "文档";
  }

  let loadSeq = 0;

  $effect(() => {
    const lib = p.lib;
    const rel = p.rel || "";
    const key = p.key || "";
    const _reload = p.reload ?? 0;
    const seq = ++loadSeq;

    loading = true;
    error = "";
    filter = "";
    excluded = [];

    const libObj = getLibrary?.(lib);
    const libTitle = libObj?.title || lib;
    const kind = libObj?.kind || "";

    api
      .openedSet(lib)
      .then((o) => {
        if (seq !== loadSeq) return;
        seen = { books: new Set(o.books || []), paths: new Set(o.paths || []) };
      })
      .catch(() => {});

    (async () => {
      try {
        if (kind === "folder" || kind === "notes") {
          const directory = kind === "folder" ? await api.folderDirectory(lib, rel) : null;
          const list = directory?.nodes ?? await api.folderChildren(lib, rel);
          if (seq !== loadSeq) return;
          excluded = directory?.excluded ?? [];
          const nextCrumbs = [{ title: libTitle, rel: "", key: "" }];
          if (kind === "folder" && rel) {
            const parts = rel.split("/").filter(Boolean);
            let acc = "";
            for (const part of parts) {
              acc = acc ? `${acc}/${part}` : part;
              nextCrumbs.push({ title: part, rel: acc, key: "" });
            }
          }
          crumbs = nextCrumbs;
          items = list.map((node, i) => ({ node, childKey: key ? `${key}/${i}` : `/${i}` }));
          const curTitle = nextCrumbs.at(-1)?.title || libTitle;
          const tooltip = nextCrumbs.map((c) => c.title).join(" › ");
          ontitle?.(curTitle, tooltip);
        } else if (kind === "zim") {
          if (seq !== loadSeq) return;
          crumbs = [{ title: libTitle, rel: "", key: "" }];
          items = [];
          ontitle?.(libTitle, libTitle);
        } else {
          const root = await api.tree(lib);
          if (seq !== loadSeq) return;
          const nextCrumbs = [{ title: libTitle, rel: "", key: "" }];
          let list = root;
          let accKey = "";
          for (const seg of key.split("/").filter(Boolean)) {
            const idx = Number(seg);
            const node = list?.[idx];
            if (!node) {
              throw new Error("目录不存在或结构已变更");
            }
            accKey += `/${idx}`;
            nextCrumbs.push({ title: node.title, rel: node.rel || "", key: accKey });
            list = node.children || [];
          }
          crumbs = nextCrumbs;
          items = list.map((node, i) => ({ node, childKey: `${accKey}/${i}` }));
          const curTitle = nextCrumbs.at(-1)?.title || libTitle;
          const tooltip = nextCrumbs.map((c) => c.title).join(" › ");
          ontitle?.(curTitle, tooltip);
        }
      } catch (e) {
        if (seq !== loadSeq) return;
        error = String(e);
        items = [];
      } finally {
        if (seq === loadSeq) loading = false;
      }
    })();
  });

  const filtered = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return items;
    return items.filter(({ node }) => `${node.title || ""} ${node.book_id || ""} ${node.rel || ""}`.toLowerCase().includes(q));
  });

  const counts = $derived.by(() => {
    let dirs = 0;
    let docs = 0;
    for (const { node } of items) {
      if (isDir(node)) dirs += 1;
      else if (isDoc(node)) docs += 1;
    }
    return { dirs, docs };
  });

  function openItem(item, e) {
    const n = item.node;
    if (isUnsupported(n)) return;
    if (isDoc(n)) {
      onopen?.({ lib: p.lib, bookId: n.book_id ?? null, path: n.path ?? n.start ?? null });
      return;
    }
    const newTab = Boolean(e?.ctrlKey || e?.metaKey || e?.button === 1);
    onnavigate?.({ lib: p.lib, rel: n.rel || "", key: item.childKey, title: n.title }, { newTab });
  }

  function clickCrumb(c, idx, e) {
    if (idx === crumbs.length - 1) return;
    const newTab = Boolean(e?.ctrlKey || e?.metaKey || e?.button === 1);
    onnavigate?.({ lib: p.lib, rel: c.rel || "", key: c.key || "", title: c.title }, { newTab });
  }

  function middleClick(fn, e) {
    if (e.button === 1) {
      e.preventDefault();
      fn(e);
    }
  }

  const libKind = $derived(getLibrary?.(p.lib)?.kind || "");
</script>

<div class="folder-panel">
  <div class="view-bar">
    <nav class="crumbs" aria-label="目录路径">
      {#each crumbs as c, i}
        {#if i > 0}<span class="sep">›</span>{/if}
        <button
          type="button"
          class="crumb"
          class:last={i === crumbs.length - 1}
          disabled={i === crumbs.length - 1}
          onclick={(e) => clickCrumb(c, i, e)}
          onauxclick={(e) => middleClick((ev) => clickCrumb(c, i, ev), e)}
          title={c.title}
        >{c.title}</button>
      {/each}
    </nav>
    {#if onlocate}
      <button type="button" class="tool-btn" onclick={onlocate} title="在目录树中定位" aria-label="在目录树中定位">
        <Icon name="locate" size={16} />
      </button>
    {/if}
    {#if libKind === "folder" || libKind === "notes"}
      <button type="button" class="tool-btn" onclick={() => (p.reload = (p.reload ?? 0) + 1)} title="刷新目录" aria-label="刷新目录">
        <Icon name="snapshot" size={15} />
      </button>
    {/if}
    <span class="grow"></span>
    <span class="muted summary">
      {#if counts.dirs && counts.docs}
        {counts.dirs} 个文件夹 · {counts.docs} 篇文档
      {:else if counts.dirs}
        {counts.dirs} 个文件夹
      {:else if counts.docs}
        {counts.docs} 篇文档
      {:else if !loading}
        0 项
      {/if}
    </span>
    {#if items.length > 0}
      <input class="filter" type="search" placeholder="筛选当前目录…" bind:value={filter} />
    {/if}
    <div class="seg" role="group" aria-label="展示方式">
      <button type="button" class:on={viewMode === "grid"} onclick={() => setViewMode("grid")} title="卡片视图">卡片</button>
      <button type="button" class:on={viewMode === "list"} onclick={() => setViewMode("list")} title="列表视图">列表</button>
    </div>
  </div>

  <div class="scroll">
    {#if excluded.length}
      <details class="excluded">
        <summary>{excluded.length} 个文件被收录规则排除</summary>
        <ul>
          {#each excluded as file (file.path)}
            <li>{file.path.split("/").at(-1)} <span class="muted">· {file.reason}</span></li>
          {/each}
        </ul>
        {#if onrules}<button type="button" onclick={() => onrules(p.lib)}>修改收录规则</button>{/if}
      </details>
    {/if}
    {#if error}
      <p class="error state-msg">{error}</p>
    {:else if loading && !items.length}
      <p class="muted state-msg">载入目录…</p>
    {:else if !items.length}
      <div class="empty">
        <Icon name="folder-open" size={36} />
        {#if libKind === "zim"}
          <p class="muted">ZIM 知识库不包含目录树，请使用标题快搜或全文搜索浏览文章。</p>
        {:else if excluded.length}
          <p class="muted">此目录没有符合当前收录规则的文件。</p>
        {:else}
          <p class="muted">此目录暂无内容</p>
        {/if}
      </div>
    {:else if !filtered.length}
      <p class="muted state-msg">没有匹配「{filter}」的条目。</p>
    {:else if viewMode === "grid"}
      <div class="grid">
        {#each filtered as item (item.childKey)}
          {@const n = item.node}
          {@const dir = isDir(n)}
          {@const unsupported = isUnsupported(n)}
          {@const opened = wasOpened(n)}
          <button
            type="button"
            class="card"
            class:is-dir={dir}
            class:seen={opened}
            class:unsupported={unsupported}
            disabled={unsupported}
            onclick={(e) => openItem(item, e)}
            onauxclick={(e) => middleClick((ev) => openItem(item, ev), e)}
            title={unsupported ? `内容尚未接入：${n.href}` : n.title}
          >
            <span class="card-head">
              <span class="card-icon" class:file={!dir} class:seen={opened}>
                <Icon name={itemIcon(n)} size={20} />
              </span>
              <span class="card-title">{n.title}</span>
            </span>
            <span class="card-meta">{itemMeta(n)}</span>
          </button>
        {/each}
      </div>
    {:else}
      <ul class="list">
        {#each filtered as item (item.childKey)}
          {@const n = item.node}
          {@const dir = isDir(n)}
          {@const unsupported = isUnsupported(n)}
          {@const opened = wasOpened(n)}
          <li>
            <button
              type="button"
              class="list-row"
              class:is-dir={dir}
              class:seen={opened}
              class:unsupported={unsupported}
              disabled={unsupported}
              onclick={(e) => openItem(item, e)}
              onauxclick={(e) => middleClick((ev) => openItem(item, ev), e)}
              title={unsupported ? `内容尚未接入：${n.href}` : n.title}
            >
              <span class="row-icon" class:file={!dir} class:seen={opened}>
                <Icon name={itemIcon(n)} size={17} />
              </span>
              <span class="row-title">{n.title}</span>
              <span class="row-meta">{itemMeta(n)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .excluded { margin: 12px 20px 0; padding: 8px 12px; border: 1px solid var(--line); border-radius: 6px; font-size: 13px; }
  .excluded summary { cursor: pointer; color: var(--accent); }
  .excluded ul { overflow-wrap: anywhere; padding-left: 20px; }
  .folder-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--paper);
  }
  .view-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--paper-2, var(--paper));
    flex: none;
    min-height: 38px;
    flex-wrap: wrap;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
    flex-wrap: wrap;
  }
  .sep {
    color: var(--muted);
    font-size: 13px;
    padding: 0 2px;
    user-select: none;
  }
  .crumb {
    border: none;
    background: none;
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 13.5px;
    color: var(--muted);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .crumb:hover:not(:disabled) {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .crumb.last {
    color: var(--ink);
    font-weight: 700;
    cursor: default;
  }
  .tool-btn {
    border: none;
    background: none;
    padding: 4px;
    border-radius: 4px;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .tool-btn:hover {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .grow { flex: 1; }
  .summary {
    font-size: 12px;
    white-space: nowrap;
  }
  .filter {
    width: 170px;
    max-width: 34vw;
    padding: 3px 8px;
    font-size: 12.5px;
    border-radius: 5px;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--line);
    border-radius: 6px;
    overflow: hidden;
    flex: none;
  }
  .seg button {
    border: none;
    background: none;
    padding: 3px 9px;
    font-size: 12px;
    color: var(--muted);
  }
  .seg button.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 18px 20px 32px;
  }
  .state-msg {
    margin: 24px 0;
    text-align: center;
    font-size: 14px;
  }
  .empty {
    margin: 48px auto;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    color: var(--muted);
    text-align: center;
  }
  .empty p { margin: 0; font-size: 14px; }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }
  .card {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: 10px;
    min-height: 82px;
    text-align: left;
    padding: 12px 14px 10px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--paper);
    transition: border-color 0.12s ease, background-color 0.12s ease, box-shadow 0.12s ease;
    min-width: 0;
  }
  .card:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06);
  }
  .card.is-dir {
    background: color-mix(in srgb, var(--accent) 4%, var(--paper));
  }
  .card.is-dir:hover:not(:disabled) {
    background: var(--accent-soft);
  }
  .card.unsupported {
    opacity: 0.5;
    cursor: default;
  }
  .card-head {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-width: 0;
  }
  .card-icon {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-top: 1px;
    color: #4f9de8;
  }
  .card-icon.file { color: #6aa7e8; }
  .card-icon.seen { color: var(--note); }
  .card-title {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    line-height: 1.45;
    color: var(--ink);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: break-word;
  }
  .card.is-dir .card-title { font-weight: 600; }
  .card.seen .card-title { color: var(--note); }
  .card-meta {
    align-self: flex-end;
    font-size: 11.5px;
    line-height: 1;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .list-row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    text-align: left;
    border-bottom: 1px dashed var(--line);
  }
  .list-row:hover:not(:disabled) {
    background: var(--accent-soft);
  }
  .list-row.unsupported {
    opacity: 0.5;
    cursor: default;
  }
  .row-icon {
    flex: none;
    display: inline-flex;
    color: #4f9de8;
  }
  .row-icon.file { color: #6aa7e8; }
  .row-icon.seen { color: var(--note); }
  .row-title {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .list-row.is-dir .row-title { font-weight: 600; }
  .list-row.seen .row-title { color: var(--note); }
  .row-meta {
    flex: none;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }
</style>
