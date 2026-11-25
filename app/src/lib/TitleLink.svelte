<script>
  // The title at the left of a reader's tool bar. With `onlocate` it is a link:
  // the locate icon and the title together show the document in the tree.
  // Right-click opens the document tab's own menu, at the pointer.
  import Icon from "./Icon.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import { getTabMenuItems } from "./workspace.svelte.js";

  // `text`: what is shown when it is not the whole title (the tool bar next to it may already say part of it)
  let { title, text = title, onlocate = null } = $props();
  let menu = $state(null);

  function tabMenu(e) {
    const items = getTabMenuItems(e.currentTarget);
    if (!items?.length) return;
    e.preventDefault();
    e.stopPropagation();
    menu = { x: e.clientX, y: e.clientY, items };
  }
</script>

{#if onlocate}
  <button class="title-link" onclick={onlocate} oncontextmenu={tabMenu} title="在目录树中定位：{title}（右键：标签菜单）" aria-label="在目录树中定位">
    <Icon name="locate" size={16} />{#if text}<span>{text}</span>{/if}
  </button>
{:else}
  <strong class="title-plain">{title}</strong>
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .title-link {
    display: inline-flex; align-items: center; gap: 5px; max-width: 22em; min-width: 0; flex: none;
    border: none; background: none; padding: 2px 6px; border-radius: 4px; font-weight: 700; color: inherit;
  }
  .title-link span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .title-link:hover { background: var(--accent-soft); color: var(--accent); }
  .title-plain { white-space: nowrap; }
</style>
