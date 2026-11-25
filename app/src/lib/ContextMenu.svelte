<script>
  // A small context menu at (x, y). Items: { label, action, disabled } or
  // "separator". Closes on choice, outside click or Escape.
  import { onMount } from "svelte";

  let { x, y, items, onclose } = $props();
  let el = $state();
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    // Re-clamp if an item's label updates asynchronously (e.g. pinyin preview).
    for (const it of items) {
      if (it && typeof it === "object") void it.label;
    }
    if (!el) return;
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.min(x, window.innerWidth - r.width - 8),
      top: Math.min(y, window.innerHeight - r.height - 8),
    };
  });

  onMount(() => {
    // keep the menu inside the window
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.min(x, window.innerWidth - r.width - 8),
      top: Math.min(y, window.innerHeight - r.height - 8),
    };
    el.querySelector("button:not([disabled])")?.focus();
    const away = (e) => {
      if (!el.contains(e.target)) onclose();
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

  function choose(item) {
    onclose();
    item.action?.();
  }
</script>

<div class="menu" role="menu" bind:this={el} style="left: {pos.left}px; top: {pos.top}px">
  {#each items as item}
    {#if item === "separator"}
      <div class="sep"></div>
    {:else}
      <button role="menuitem" class:has-sub={Boolean(item.sub)} disabled={item.disabled} onclick={() => choose(item)}>
        <span class="row">
          <span class="label">{item.label}</span>
          {#if item.badge}<span class="badge">{item.badge}</span>{/if}
        </span>
        {#if item.sub}<span class="sub">{item.sub}</span>{/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .menu {
    position: fixed; z-index: 1000; min-width: 160px; max-width: min(360px, calc(100vw - 16px));
    max-height: min(60vh, 420px); overflow-y: auto; padding: 4px;
    background: var(--paper); border: 1px solid var(--line); border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18); display: flex; flex-direction: column;
  }
  button { border: none; background: none; text-align: left; padding: 6px 12px; border-radius: 4px; font-size: 14px; display: flex; flex-direction: column; gap: 2px; }
  button.has-sub { padding: 7px 12px; }
  button:hover:not([disabled]), button:focus-visible { background: var(--accent-soft); outline: none; }
  button[disabled] { color: var(--muted); cursor: default; }
  .row { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge {
    flex: none; font-size: 11px; line-height: 1.3; padding: 1px 5px; border-radius: 4px;
    background: var(--accent-soft); color: var(--accent);
  }
  .sub { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sep { height: 1px; background: var(--line); margin: 4px 6px; }
</style>
