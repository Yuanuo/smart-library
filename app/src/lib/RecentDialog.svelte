<script>
  // The reading history: every document opened so far, newest first, with
  // the position where it was left. Click to open there.
  import { onMount, untrack } from "svelte";
  import * as api from "./api.js";

  // `embedded`: shown in a window of its own (no backdrop, stays open);
  // `refresh` changes when the data may have changed
  let { onopen, onclose, onpopout = null, embedded = false, refresh = 0 } = $props();
  let filter = $state("");
  let items = $state([]);
  let sel = $state(0);
  let input = $state();
  let list = $state();
  let error = $state("");
  let timer = null;

  async function load() {
    try {
      items = await api.recentList(filter, 300);
      sel = 0;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    refresh;
    untrack(load);
  });
  onMount(() => input?.focus());

  function changed() {
    clearTimeout(timer);
    timer = setTimeout(load, 150);
  }

  function ago(t) {
    const s = Math.max(0, Date.now() / 1000 - t);
    if (s < 60) return "刚刚";
    if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
    if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
    if (s < 86400 * 30) return `${Math.floor(s / 86400)} 天前`;
    return new Date(t * 1000).toLocaleDateString("zh-CN");
  }

  function go(it) {
    onopen(it);
    if (!embedded) onclose();
  }

  async function remove(it) {
    await api.recentRemove(it.lib, it.path).catch(() => {});
    items = items.filter((x) => x !== it);
  }

  async function clearAll() {
    await api.recentClear().catch(() => {});
    items = [];
  }

  function key(e) {
    if (e.key === "Escape" && !embedded) onclose();
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = Math.min(items.length - 1, sel + 1);
      list?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, sel - 1);
      list?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && items[sel]) {
      e.preventDefault();
      go(items[sel]);
    }
  }
</script>

<div class="backdrop" class:embedded role="presentation" onmousedown={(e) => !embedded && e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="最近访问" tabindex="-1" onkeydown={key}>
    <div class="head">
      <strong>最近访问</strong>
      <input type="search" placeholder="按标题筛选" bind:value={filter} bind:this={input} oninput={changed} />
      <button class="link" onclick={clearAll} disabled={!items.length}>清空历史</button>
      {#if onpopout && !embedded}<button class="link" onclick={onpopout} title="在独立窗口中持续显示，可以和主窗口同时操作">新窗口</button>{/if}
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <ul bind:this={list}>
      {#each items as it, i (it.lib + "|" + it.path)}
        <li class:on={i === sel}>
          <button class="row" onclick={() => go(it)} onmousemove={() => (sel = i)}>
            <span class="title">{it.title}</span>
            <span class="meta">{it.lib_title}{it.lb ? ` · ${it.lb}` : ""}</span>
            <span class="when">{ago(it.opened)}{it.visits > 1 ? ` · ${it.visits} 次` : ""}</span>
          </button>
          <button class="x" title="从历史中删除" aria-label="从历史中删除" onclick={() => remove(it)}>×</button>
        </li>
      {:else}
        <li class="empty muted">{filter ? "没有匹配的记录。" : "还没有打开过文档。"}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 900; background: rgba(0, 0, 0, 0.25);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 10vh;
  }
  .box {
    width: min(760px, calc(100vw - 32px)); max-height: 72vh; display: flex; flex-direction: column;
    background: var(--paper); border-radius: 10px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3); outline: none;
  }
  .backdrop.embedded { position: static; background: none; padding: 0; display: block; height: 100%; }
  .embedded .box { width: 100%; max-height: none; height: 100%; border-radius: 0; box-shadow: none; }
  .head { display: flex; gap: 12px; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--line); }
  .head input { flex: 1; }
  ul { list-style: none; margin: 0; padding: 4px; overflow: auto; }
  li { display: flex; align-items: center; border-radius: 6px; }
  li.on { background: var(--accent-soft); }
  .row {
    flex: 1; min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 14px; align-items: baseline;
    text-align: left; border: none; background: none; padding: 7px 10px;
  }
  .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta, .when { font-size: 12px; color: var(--muted); white-space: nowrap; }
  .x { border: none; background: none; color: var(--muted); padding: 2px 10px; font-size: 16px; }
  .x:hover { color: var(--hl); }
  .empty { padding: 18px; text-align: center; }
  .error { color: var(--hl); padding: 0 16px; }
</style>
