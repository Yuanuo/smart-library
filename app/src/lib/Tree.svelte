<script>
  // One level of the navigation tree; children are rendered only when open,
  // so trees with thousands of leaves stay light.
  import Tree from "./Tree.svelte";

  let { nodes, onopen, depth = 0, current = null } = $props();
  let open = $state({});

  function toggle(i) {
    open[i] = !open[i];
  }
</script>

<ul class:root={depth === 0}>
  {#each nodes as node, i}
    {@const hasChildren = node.children.length > 0}
    <li>
      <div class="row" style="padding-left: {depth * 14 + 4}px">
        {#if hasChildren}
          <button class="twisty" onclick={() => toggle(i)} aria-label={open[i] ? "收起" : "展开"}>
            {open[i] ? "▾" : "▸"}
          </button>
        {:else}
          <span class="twisty"></span>
        {/if}
        {#if node.book_id || node.path}
          <button
            class="leaf"
            class:current={current && (current === node.book_id || current === node.path)}
            onclick={() => onopen(node)}
            title={node.title}>{node.title}</button>
        {:else if node.href}
          <span class="leaf unsupported" title="内容尚未接入：{node.href}">{node.title}</span>
        {:else}
          <button class="folder" onclick={() => toggle(i)} title={node.title}>{node.title}</button>
        {/if}
      </div>
      {#if hasChildren && open[i]}
        <Tree nodes={node.children} {onopen} depth={depth + 1} {current} />
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 2px; min-height: 26px; }
  .twisty {
    width: 18px; flex: none; border: none; background: none; padding: 0;
    color: var(--muted); font-size: 12px;
  }
  .leaf, .folder {
    border: none; background: none; padding: 2px 4px; text-align: left;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; flex: 1; min-width: 0;
    border-radius: 4px;
  }
  .folder { font-weight: 600; }
  .leaf:hover, .folder:hover { background: var(--accent-soft); }
  .leaf.current { background: var(--accent-soft); color: var(--accent); }
  .unsupported { color: var(--muted); cursor: default; }
</style>
