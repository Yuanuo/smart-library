<script>
  import ContextMenu from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";

  let { scopes, libraryTitles, onchoose, onadd, onremove, onclear } = $props();
  let menu = $state(null);
  const groups = $derived.by(() => {
    const grouped = new Map();
    for (const scope of scopes) {
      const [root, ...parts] = scope.label.split(" › ");
      if (!grouped.has(scope.lib)) {
        const title = libraryTitles.get(scope.lib) ?? (parts.length || scope.whole ? root : scope.lib);
        grouped.set(scope.lib, { title, items: [] });
      }
      grouped.get(scope.lib).items.push({ scope, label: scope.whole ? "整个知识库" : parts.join(" › ") || scope.label });
    }
    return [...grouped.values()];
  });

  function scopeMenu(event, scope) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, items: [{ label: "移除此项", action: () => onremove(scope) }] };
  }
</script>

<div class="scope" data-scope-drop onscopedrop={(event) => onadd(event.detail)}>
  <div class="scope-actions">
    <button type="button" class="add" onclick={onchoose}><Icon name="plus" size={14} /> 添加范围</button>
    <button type="button" class="link" disabled={!scopes.length} onclick={onclear}>清空范围</button>
  </div>
  <div class="scope-list">
    {#each groups as group}
      <section class="scope-group">
        <h3><Icon name="library" size={15} /> {group.title}</h3>
        <ul>
          {#each group.items as { scope, label } (scope)}
            <li oncontextmenu={(event) => scopeMenu(event, scope)}>
              <span title={scope.label}>{label}</span>
              <button type="button" class="remove" onclick={() => onremove(scope)} aria-label={`移除：${scope.label}`}>×</button>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <p class="muted empty">选择知识库或目录，也可拖入目录树中的节点。</p>
    {/each}
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .scope { display: flex; flex-direction: column; min-height: 0; border: 1px solid transparent; }
  .scope:global(.scope-over) { border-color: var(--accent); background: var(--accent-soft); }
  .scope-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 12px; padding: 10px 12px 6px; }
  .scope-actions button:disabled { opacity: .55; cursor: default; }
  .add { display: inline-flex; align-items: center; gap: 5px; color: var(--accent); }
  .scope-list { flex: 1; min-height: 0; overflow: auto; padding: 4px 12px 12px; }
  .scope-group + .scope-group { margin-top: 10px; }
  h3 { display: flex; align-items: center; gap: 6px; margin: 0 0 3px; font-size: 13px; color: var(--accent); overflow-wrap: anywhere; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; justify-content: space-between; gap: 6px; min-height: 30px; padding-left: 21px; font-size: 14px; }
  li:hover { background: var(--accent-soft); border-radius: 6px; }
  li span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .remove { flex: none; width: 28px; padding: 1px 0; border-color: transparent; background: none; color: var(--muted); }
  .remove:hover { color: var(--accent); }
  .empty { margin: 3px 0 0; font-size: 13px; line-height: 1.55; }
</style>
