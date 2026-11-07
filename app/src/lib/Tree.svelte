<script>
  // One level of the navigation tree; children are rendered only when open,
  // so trees with thousands of leaves stay light. Which nodes are open is
  // kept by the owner (`expanded`: key -> true, a key is the index path
  // `/0/3/5`), so that a node can be revealed from outside.
  import Tree from "./Tree.svelte";

  let { nodes, onopen, ontoggle, expanded, depth = 0, prefix = "", current = null } = $props();
</script>

<ul class:root={depth === 0}>
  {#each nodes as node, i}
    {@const key = `${prefix}/${i}`}
    {@const hasChildren = node.children.length > 0}
    {@const open = !!expanded[key]}
    <li>
      <div class="row" class:current={current === key} data-key={key} style="padding-left: {depth * 14 + 4}px">
        {#if hasChildren}
          <button class="twisty" onclick={() => ontoggle(key)} aria-label={open ? "收起" : "展开"}>
            {open ? "▾" : "▸"}
          </button>
        {:else}
          <span class="twisty"></span>
        {/if}
        {#if node.book_id || node.path}
          <button class="leaf" onclick={() => onopen(node)} title={node.title}>{node.title}</button>
        {:else if node.href}
          <span class="leaf unsupported" title="内容尚未接入：{node.href}">{node.title}</span>
        {:else}
          <button class="folder" onclick={() => ontoggle(key)} title={node.title}>{node.title}</button>
        {/if}
      </div>
      {#if hasChildren && open}
        <Tree nodes={node.children} {onopen} {ontoggle} {expanded} depth={depth + 1} prefix={key} {current} />
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 2px; min-height: 26px; border-radius: 4px; }
  .row.current { background: var(--accent-soft); }
  .row.current .leaf, .row.current .folder { color: var(--accent); }
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
  .unsupported { color: var(--muted); cursor: default; }
</style>
