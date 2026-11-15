<script>
  // Collections, bookmarks and tags in one place: click a tag to list all
  // that carry it; filter by words; look at everything or at the document
  // being read. Click an entry to go back to its place (when its library is
  // still bound).
  import { untrack } from "svelte";
  import * as api from "./api.js";
  import TagDialog from "./TagDialog.svelte";

  // `doc`: { lib, path, bookId, title } of the tab being read, or null
  // `embedded`: shown in a window of its own (no backdrop, stays open);
  // `refresh` changes when the data may have changed
  let { doc = null, onopen, onclose, onpopout = null, embedded = false, refresh = 0 } = $props();

  const KINDS = [
    [null, "全部"],
    ["clip", "收藏"],
    ["bookmark", "书签"],
    ["node", "标签"],
  ];
  const KIND_LABEL = { clip: "收藏", bookmark: "书签", node: "标签" };

  let kind = $state(null);
  let thisDoc = $state(false);
  let tag = $state(null);
  let filter = $state("");
  let items = $state([]);
  let tags = $state([]);
  let sel = $state(0);
  let error = $state("");
  let editing = $state(null);
  let input = $state();
  let timer = null;

  async function load() {
    try {
      const f = { tag, text: filter, kinds: kind ? [kind] : [], limit: 500 };
      if (thisDoc && doc) Object.assign(f, { lib: doc.lib, path: doc.path, bookId: doc.bookId });
      [items, tags] = await Promise.all([api.markList(f), api.markTags()]);
      sel = 0;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    refresh;
    doc?.path;
    untrack(load);
  });
  $effect(() => {
    input?.focus();
  });

  function changed() {
    clearTimeout(timer);
    timer = setTimeout(load, 150);
  }

  function setKind(k) {
    kind = k;
    load();
  }

  function setTag(t) {
    tag = tag === t ? null : t;
    load();
  }

  function when(t) {
    return new Date(t * 1000).toLocaleDateString("zh-CN");
  }

  function go(m) {
    if (!m.available) return;
    onopen(m);
    if (!embedded) onclose();
  }

  async function remove(m) {
    await api.markDelete(m.id).catch(() => {});
    load();
  }

  function key(e) {
    if (e.target === input || !e.target.closest?.("input, textarea")) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        sel = Math.min(items.length - 1, sel + 1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        sel = Math.max(0, sel - 1);
      } else if (e.key === "Enter" && items[sel]) {
        e.preventDefault();
        go(items[sel]);
      }
    }
    if (e.key === "Escape" && !editing && !embedded) onclose();
  }
</script>

<div class="backdrop" class:embedded role="presentation" onmousedown={(e) => !embedded && e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="收藏与标签" tabindex="-1" onkeydown={key}>
    <div class="head">
      <strong>收藏与标签</strong>
      {#if onpopout && !embedded}<button class="link" onclick={onpopout} title="在独立窗口中持续显示，可以和主窗口同时操作">新窗口</button>{/if}
      <div class="seg" role="group" aria-label="类型">
        {#each KINDS as [k, label]}
          <button class:on={kind === k} onclick={() => setKind(k)}>{label}</button>
        {/each}
      </div>
      <label class="check" class:off={!doc} title={doc ? doc.title : "还没有打开过阅读器"}>
        <input type="checkbox" bind:checked={thisDoc} disabled={!doc} onchange={load} /> 只看当前文档
      </label>
      <input type="search" placeholder="筛选：文字、备注、标题、标签" bind:value={filter} bind:this={input} oninput={changed} />
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="body">
      <aside class="tags" aria-label="标签">
        {#each tags as t (t.tag)}
          <button class="tag" class:on={tag === t.tag} onclick={() => setTag(t.tag)}>
            <span>{t.tag}</span><span class="n">{t.count}</span>
          </button>
        {:else}
          <p class="muted small">还没有标签。</p>
        {/each}
      </aside>
      <ul>
        {#each items as m, i (m.id)}
          <li class:on={i === sel} class:gone={!m.available}>
            <button class="row" onclick={() => go(m)} onmousemove={() => (sel = i)}
              title={m.available ? "" : "这个知识库已经解除绑定，不能打开"}>
              <span class="top">
                <span class="kind k-{m.kind}">{KIND_LABEL[m.kind]}</span>
                <span class="doc">{m.doc_title}</span>
                <span class="meta">{m.lib_title}{m.author ? ` · ${m.author}` : ""}{m.lb ? ` · ${m.lb}` : ""}</span>
                <span class="when">{when(m.created)}</span>
              </span>
              {#if m.quote}<span class="quote">{m.quote}</span>{/if}
              {#if m.note}<span class="note">{m.note}</span>{/if}
              {#if m.tags.length}
                <span class="tl">{#each m.tags as t}<span class="pill">{t}</span>{/each}</span>
              {/if}
            </button>
            <span class="acts">
              <button class="link" onclick={() => (editing = m)}>编辑</button>
              <button class="link del" onclick={() => remove(m)}>删除</button>
            </span>
          </li>
        {:else}
          <li class="empty muted">{filter || tag || kind || thisDoc ? "没有匹配的记录。" : "还没有收藏、书签或标签。在阅读器里选中文字点右键可以添加。"}</li>
        {/each}
      </ul>
    </div>
  </div>
</div>

{#if editing}
  <TagDialog
    heading={editing.kind === "node" ? `标签：${editing.doc_title}` : "编辑"}
    subtitle={`${editing.doc_title} · ${editing.lib_title}`}
    quote={editing.quote}
    withNote={editing.kind !== "node"}
    tags={editing.tags.join(", ")}
    note={editing.note}
    onok={async (t, n) => {
      await api.markUpdate(editing.id, editing.kind === "node" ? null : n, t);
      await load();
    }}
    onclose={() => (editing = null)} />
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 900; background: rgba(0, 0, 0, 0.25);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 7vh;
  }
  .box {
    width: min(960px, calc(100vw - 32px)); height: min(76vh, 760px); display: flex; flex-direction: column;
    background: var(--paper); border-radius: 10px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3); outline: none;
  }
  .backdrop.embedded { position: static; background: none; padding: 0; display: block; height: 100%; }
  .embedded .box { width: 100%; height: 100%; border-radius: 0; box-shadow: none; }
  .head { display: flex; gap: 12px; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .head input[type="search"] { flex: 1; min-width: 180px; }
  .check.off { opacity: 0.5; }
  .check { display: flex; gap: 4px; align-items: center; font-size: 13px; white-space: nowrap; }
  .seg { display: flex; }
  .seg button { border-radius: 0; margin-left: -1px; padding: 3px 12px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 6px 6px 0; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .body { flex: 1; display: flex; min-height: 0; }
  .tags { width: 190px; flex: none; overflow: auto; padding: 8px; border-right: 1px solid var(--line); background: var(--panel); border-radius: 0 0 0 10px; }
  .tag { display: flex; justify-content: space-between; width: 100%; border: none; background: none; padding: 4px 8px; text-align: left; border-radius: 6px; }
  .tag:hover { background: var(--accent-soft); }
  .tag.on { background: var(--accent); color: var(--paper); }
  .tag .n { opacity: 0.7; font-size: 12px; }
  .small { font-size: 13px; padding: 4px 8px; }
  ul { flex: 1; list-style: none; margin: 0; padding: 6px; overflow: auto; min-width: 0; }
  li { display: flex; gap: 6px; align-items: flex-start; border-radius: 6px; border-bottom: 1px dashed var(--line); }
  li.on { background: var(--accent-soft); }
  li.gone .row { opacity: 0.55; cursor: not-allowed; }
  .row {
    flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; text-align: left; border: none;
    background: none; padding: 8px 10px;
  }
  .top { display: flex; gap: 10px; align-items: baseline; }
  .kind { font-size: 11px; padding: 0 6px; border-radius: 8px; background: var(--accent-soft); color: var(--accent); flex: none; }
  .k-clip { background: #dcebfb; color: #2b6cb0; }
  .k-bookmark { background: #fbe9c8; color: #9a5b00; }
  .doc { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .meta, .when { font-size: 12px; color: var(--muted); white-space: nowrap; }
  .meta { overflow: hidden; text-overflow: ellipsis; flex: 1; min-width: 0; }
  .quote { color: var(--ink); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 4.8em; overflow: hidden; }
  .note { font-size: 13px; color: var(--muted); white-space: pre-wrap; }
  .tl { display: flex; gap: 6px; flex-wrap: wrap; }
  .pill { font-size: 12px; padding: 0 8px; border: 1px solid var(--line); border-radius: 10px; color: var(--muted); }
  .acts { display: flex; flex-direction: column; gap: 4px; padding: 8px 8px 0 0; font-size: 13px; }
  .del { color: var(--hl); }
  .empty { padding: 24px; text-align: center; }
  .error { color: var(--hl); padding: 0 16px; }
</style>
