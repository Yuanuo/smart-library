<script>
  // The tree panel: one root per bound knowledge library, its tree loaded
  // when first opened. Right-click a library for its actions, or the empty
  // panel to bind a library.
  import * as api from "./api.js";
  import Tree from "./Tree.svelte";
  import ContextMenu from "./ContextMenu.svelte";

  let { libraries, progress, onopen, onbind, onrebind, onunbind, onindex } = $props();

  let open = $state({}); // lib id -> expanded
  let nodes = $state({}); // lib id -> tree nodes
  let loading = $state({});
  let errors = $state({});
  let menu = $state(null); // { x, y, items }

  // a library bound again with another tree: reload it
  let trees = {};
  $effect(() => {
    for (const l of libraries) {
      if (trees[l.id] && trees[l.id] !== l.tree) {
        delete nodes[l.id];
        if (open[l.id]) load(l.id);
      }
      trees[l.id] = l.tree;
    }
  });

  async function load(id) {
    loading[id] = true;
    errors[id] = "";
    try {
      nodes[id] = await api.tree(id);
    } catch (e) {
      errors[id] = String(e);
    } finally {
      loading[id] = false;
    }
  }

  function toggle(l) {
    open[l.id] = !open[l.id];
    if (open[l.id] && !nodes[l.id]) load(l.id);
  }

  function badge(l) {
    if (l.error) return { text: "数据不可用", warn: true };
    const p = progress[l.id];
    if (l.building || p) return { text: p ? `建索引 ${Math.floor((100 * p.done) / Math.max(1, p.total))}%` : "建索引…" };
    if (!l.index_ready) return { text: "未建索引", warn: true };
    if (l.index_data_version && l.data_version && l.index_data_version !== l.data_version)
      return { text: "索引过期", warn: true };
    return null;
  }

  function libMenu(e, l) {
    e.preventDefault();
    e.stopPropagation();
    const building = l.building || !!progress[l.id];
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: open[l.id] ? "收起" : "展开", action: () => toggle(l) },
        "separator",
        { label: l.index_ready ? "重建索引" : "建立索引", action: () => onindex(l.id), disabled: building || !!l.error },
        ...(l.kind === "cbeta" ? [{ label: "更换目录树…", action: () => onrebind(l) }] : []),
        { label: "从同一数据包绑定其他知识库…", action: () => onrebind(l) },
        "separator",
        { label: "解除绑定", action: () => onunbind(l), disabled: building },
      ],
    };
  }

  function panelMenu(e) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, items: [{ label: "绑定知识库…", action: onbind }] };
  }
</script>

<div class="panel" role="tree" aria-label="知识库" oncontextmenu={panelMenu}>
  {#each libraries as l (l.id)}
    {@const b = badge(l)}
    <div class="lib">
      <div class="root" role="treeitem" aria-expanded={!!open[l.id]} aria-selected="false" tabindex="0"
        oncontextmenu={(e) => libMenu(e, l)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggle(l))}>
        <button class="twisty" onclick={() => toggle(l)} tabindex="-1" aria-hidden="true">{open[l.id] ? "▾" : "▸"}</button>
        <button class="title" onclick={() => toggle(l)} tabindex="-1" title={`${l.title}\n${l.source}`}>{l.title}</button>
        {#if b}<span class="badge" class:warn={b.warn}>{b.text}</span>{/if}
        <button class="more" title="更多操作" aria-label="{l.title} 的操作" onclick={(e) => libMenu(e, l)}>⋯</button>
      </div>
      {#if open[l.id]}
        {#if loading[l.id]}
          <p class="muted note">载入中…</p>
        {:else if errors[l.id]}
          <p class="error note">{errors[l.id]}</p>
        {:else if nodes[l.id]}
          <Tree nodes={nodes[l.id]} depth={1} onopen={(node) => onopen(l, node)} />
        {/if}
      {/if}
    </div>
  {:else}
    <div class="empty">
      <p>还没有绑定知识库。</p>
      <button class="primary" onclick={onbind}>绑定知识库…</button>
      <p class="muted small">也可以在这里点右键绑定。</p>
    </div>
  {/each}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .panel { min-height: 100%; padding-bottom: 40px; }
  .lib { margin-bottom: 4px; }
  .root {
    display: flex; align-items: center; gap: 2px; min-height: 30px; border-radius: 6px; padding-right: 2px;
  }
  .root:hover, .root:focus-visible { background: var(--accent-soft); outline: none; }
  .twisty { width: 18px; flex: none; border: none; background: none; padding: 0; color: var(--muted); font-size: 12px; }
  .title {
    flex: 1; min-width: 0; border: none; background: none; text-align: left; padding: 2px 4px;
    font-weight: 700; color: var(--accent); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .badge { font-size: 11px; color: var(--muted); white-space: nowrap; }
  .badge.warn { color: var(--hl); }
  .more { border: none; background: none; padding: 0 6px; color: var(--muted); }
  .more:hover { color: var(--accent); }
  .note { font-size: 13px; padding-left: 24px; }
  .error { color: var(--hl); }
  .empty { padding: 24px 12px; text-align: center; }
  .small { font-size: 13px; }
</style>
