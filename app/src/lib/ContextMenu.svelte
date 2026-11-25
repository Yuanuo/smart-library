<script>
  // A compact context menu at (x, y). Items:
  //   { label, action, disabled, badge, sub, children } or "separator".
  // Closes on choice, outside click or Escape.
  import { onMount, tick } from "svelte";

  let { x, y, items, onclose } = $props();
  let el = $state();
  let subEl = $state();
  let pos = $state({ left: 0, top: 0 });
  let openSub = $state(null); // { index, items, top, left }

  function clampPos(rect, px, py) {
    return {
      left: Math.max(8, Math.min(px, window.innerWidth - rect.width - 8)),
      top: Math.max(8, Math.min(py, window.innerHeight - rect.height - 8)),
    };
  }

  $effect(() => {
    // Re-clamp if an item's label updates asynchronously (e.g. pinyin preview).
    for (const it of items) {
      if (it && typeof it === "object") void it.label;
    }
    if (!el) return;
    pos = clampPos(el.getBoundingClientRect(), x, y);
  });

  onMount(() => {
    pos = clampPos(el.getBoundingClientRect(), x, y);
    el.querySelector("button:not([disabled])")?.focus();
    const away = (e) => {
      if (!el?.contains(e.target) && !subEl?.contains(e.target)) onclose();
    };
    const key = (e) => {
      if (e.key === "Escape") onclose();
    };
    setTimeout(() => window.addEventListener("mousedown", away), 0);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", key);
    };
  });

  async function showSub(index, item, btn) {
    if (!item.children?.length || item.disabled) {
      openSub = null;
      return;
    }
    const r = btn.getBoundingClientRect();
    openSub = { index, items: item.children, left: r.right - 2, top: r.top - 4 };
    await tick();
    if (!subEl || openSub?.index !== index) return;
    const sr = subEl.getBoundingClientRect();
    let left = r.right - 2;
    if (left + sr.width > window.innerWidth - 8) {
      left = Math.max(8, r.left - sr.width + 2);
    }
    const top = Math.max(8, Math.min(r.top - 4, window.innerHeight - sr.height - 8));
    openSub = { index, items: item.children, left, top };
  }

  function choose(item, index, e) {
    if (item.children?.length) {
      showSub(index, item, e.currentTarget);
      return;
    }
    onclose();
    item.action?.();
  }
</script>

<div class="menu" role="menu" bind:this={el} style="left: {pos.left}px; top: {pos.top}px">
  {#each items as item, i}
    {#if item === "separator"}
      <div class="sep"></div>
    {:else}
      <button
        role="menuitem"
        aria-haspopup={item.children?.length ? "menu" : undefined}
        aria-expanded={item.children?.length ? openSub?.index === i : undefined}
        class:has-sub={Boolean(item.sub)}
        class:sub-open={openSub?.index === i}
        disabled={item.disabled}
        onmouseenter={(e) => (item.children?.length ? showSub(i, item, e.currentTarget) : (openSub = null))}
        onclick={(e) => choose(item, i, e)}>
        <span class="row">
          <span class="label">{item.label}</span>
          {#if item.badge}<span class="badge">{item.badge}</span>{/if}
          {#if item.children?.length}<span class="arrow" aria-hidden="true">›</span>{/if}
        </span>
        {#if item.sub}<span class="sub">{item.sub}</span>{/if}
      </button>
    {/if}
  {/each}
</div>

{#if openSub}
  <div class="menu submenu" role="menu" bind:this={subEl} style="left: {openSub.left}px; top: {openSub.top}px">
    {#each openSub.items as child}
      {#if child === "separator"}
        <div class="sep"></div>
      {:else}
        <button role="menuitem" class:has-sub={Boolean(child.sub)} disabled={child.disabled} onclick={() => (onclose(), child.action?.())}>
          <span class="row">
            <span class="label">{child.label}</span>
            {#if child.badge}<span class="badge">{child.badge}</span>{/if}
          </span>
          {#if child.sub}<span class="sub">{child.sub}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed; z-index: 1000; min-width: 176px; max-width: min(340px, calc(100vw - 16px));
    max-height: calc(100vh - 16px); overflow-y: auto; padding: 4px;
    background: var(--paper); border: 1px solid var(--line); border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.16); display: flex; flex-direction: column;
  }
  .submenu { z-index: 1001; }
  button {
    flex-shrink: 0; border: none; background: none; text-align: left;
    padding: 5px 10px; border-radius: 5px; font-size: 13px; line-height: 1.35;
    display: flex; flex-direction: column; gap: 2px; color: var(--ink);
  }
  button.has-sub { padding: 6px 10px; }
  button:hover:not([disabled]), button:focus-visible, button.sub-open { background: var(--accent-soft); outline: none; }
  button[disabled] { color: var(--muted); cursor: default; }
  .row { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .arrow { flex: none; font-size: 14px; line-height: 1; color: var(--muted); }
  .badge {
    flex: none; font-size: 11px; line-height: 1.3; padding: 1px 5px; border-radius: 4px;
    background: var(--accent-soft); color: var(--accent);
  }
  .sub { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sep { flex-shrink: 0; height: 1px; background: var(--line); margin: 4px 6px; }
</style>
