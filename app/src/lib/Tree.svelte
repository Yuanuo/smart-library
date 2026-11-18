<script>
  // One level of the navigation tree; children are rendered only when open,
  // so trees with thousands of leaves stay light. Which nodes are open is
  // kept by the owner (`expanded`: key -> true, a key is the index path
  // `/0/3/5`), so that a node can be revealed from outside.
  import Tree from "./Tree.svelte";
  import Icon from "./Icon.svelte";

  let { nodes, onopen, ontoggle, oncontext = null, ondrag = null, expanded, depth = 0, prefix = "", current = null, active = null, marked = null, seen = null, picked = null, onselect = null, onclear = null } = $props();
  const wasOpened = (n) => !!seen && ((n.book_id && seen.books.has(n.book_id)) || (n.path && seen.paths.has(n.path)));

  /** Ctrl / Shift + click picks nodes (several can be picked); a plain click opens. */
  function click(e, node, key, plain) {
    if (onselect && (e.ctrlKey || e.metaKey || e.shiftKey)) {
      e.preventDefault();
      onselect(node, key, e);
      return;
    }
    onclear?.();
    plain();
  }
</script>

<ul class:root={depth === 0}>
  {#each nodes as node, i}
    {@const key = `${prefix}/${i}`}
    {@const hasChildren = node.children.length > 0 || !!node.lazy}
    {@const open = !!expanded[key]}
    <li>
      <div class="row" class:current={current === key} class:active={active === key} class:row-current={active === key} class:marked={marked === key} class:picked={picked?.has(key)} data-key={key} data-rel={node.rel ?? undefined} data-isdir={node.rel != null && node.path == null ? "1" : undefined} style="padding-left: {depth * 14 + 4}px"
        role="presentation" oncontextmenu={(e) => oncontext?.(node, key, e)} onpointerdown={(e) => ondrag?.(node, key, e)}>
        {#if hasChildren}
          <button class="twisty" onclick={() => ontoggle(key)} aria-label={open ? "收起" : "展开"}>
            <Icon name={open ? "chevron-down" : "chevron-right"} size={14} />
          </button>
        {:else}
          <span class="gap"></span>
        {/if}
        <span class="kind" class:file={!hasChildren} class:seen={wasOpened(node)} class:dim={!node.book_id && !node.path && node.href}>
          <Icon name={node.mind ? "mindmap" : node.board ? "whiteboard" : hasChildren ? (open ? "folder-open" : "folder") : "file"} size={17} />
        </span>
        {#if node.book_id || node.path}
          <button class="leaf" class:seen={wasOpened(node)} onclick={(e) => click(e, node, key, () => { onopen(node, key); if (hasChildren) ontoggle(key); })} title={node.title}>{node.title}</button>
        {:else if node.href}
          <span class="leaf unsupported" title="内容尚未接入：{node.href}">{node.title}</span>
        {:else}
          <button class="folder" onclick={(e) => click(e, node, key, () => ontoggle(key))} title={node.title}>{node.title}</button>
        {/if}
      </div>
      {#if hasChildren && open}
        <Tree nodes={node.children} {onopen} {ontoggle} {oncontext} {ondrag} {expanded} depth={depth + 1} prefix={key} {current} {active} {marked} {seen} {picked} {onselect} {onclear} />
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul { list-style: none; margin: 0; padding: 0; }
  .row {
    display: flex; align-items: center; gap: 3px; min-height: 30px; border-radius: 4px;
    border-bottom: 1px dashed var(--line);
  }
  .kind { display: inline-flex; flex: none; color: #4f9de8; }
  .kind.file { color: #6aa7e8; }
  .kind.dim { opacity: 0.45; }
  .kind.seen { color: var(--note); }
  .leaf.seen { color: var(--note); }
  .row.current { background: var(--accent-soft); }
  .row.marked { background: color-mix(in srgb, var(--accent) 14%, transparent); outline: 1px solid color-mix(in srgb, var(--accent) 45%, transparent); outline-offset: -1px; }
  /* .row-current (app.css) gives the active row its fill and left bar */
  .row.active .leaf, .row.active .folder { color: var(--accent); font-weight: 700; }
  .row.picked { background: color-mix(in srgb, var(--accent) 24%, transparent); }
  .row.current .leaf, .row.current .folder { color: var(--accent); font-weight: 700; }
  .gap { width: 22px; height: 22px; flex: none; }
  .twisty {
    width: 22px; height: 22px; flex: none; border: none; background: none; padding: 0;
    color: var(--muted); display: inline-flex; align-items: center; justify-content: center;
    border-radius: 4px;
  }
  .leaf, .folder {
    border: none; background: none; padding: 2px 4px; text-align: left;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; flex: 1; min-width: 0;
    border-radius: 4px;
  }
  .folder { font-weight: 600; }
  .twisty:hover { background: var(--accent-soft); color: var(--accent); }
  .row:hover:not(.picked):not(.marked) { background: var(--accent-soft); }
  .unsupported { color: var(--muted); cursor: default; }
</style>
