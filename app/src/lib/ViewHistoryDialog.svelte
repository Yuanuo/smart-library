<script>
  // The view history (what the back / forward arrows walk through), newest
  // first. One row of controls on top, a list below (like 最近访问): pick a
  // row to go to that view, filter it, or clear the whole history. The view
  // being read now is marked by a bar on the left of its row.
  import { onMount } from "svelte";
  import * as workspace from "./workspace.svelte.js";

  let { onclose } = $props();
  let snap = $state(workspace.viewHistorySnapshot());
  let filter = $state("");
  let sel = $state(0);
  let error = $state("");
  let input = $state();
  let list = $state();

  const items = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return snap.items;
    return snap.items.filter((it) => `${it.title} ${it.libTitle} ${it.path ?? ""} ${it.bookId ?? ""}`.toLowerCase().includes(q));
  });

  $effect(() => {
    if (sel > items.length - 1) sel = Math.max(0, items.length - 1);
  });

  onMount(() => {
    input?.focus();
    const i = items.findIndex((it) => it.index === snap.index);
    if (i >= 0) sel = i;
    requestAnimationFrame(() => list?.querySelector(".current")?.scrollIntoView({ block: "nearest" }));
  });

  async function pick(it) {
    if (!it) return;
    // even the current row activates its document (the active tab may be a
    // search page when the dialog is opened)
    error = "";
    if (await workspace.navigateHistoryTo(it.index)) {
      onclose();
      return;
    }
    snap = workspace.viewHistorySnapshot();
    error = "这个视图已经打不开，已从记录中移除。";
  }

  function clearAll() {
    workspace.clearViewHistory();
    snap = workspace.viewHistorySnapshot();
    sel = 0;
    error = "";
  }

  function key(e) {
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!items.length) return;
      sel = e.key === "ArrowDown" ? Math.min(items.length - 1, sel + 1) : Math.max(0, sel - 1);
      list?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && items[sel]) {
      e.preventDefault();
      pick(items[sel]);
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="视图历史" tabindex="-1" onkeydown={key}>
    <div class="head">
      <strong>视图历史</strong>
      <input type="search" placeholder="按标题、路径或知识库筛选" bind:value={filter} bind:this={input} oninput={() => (sel = 0)} />
      <button class="link" onclick={clearAll} disabled={!snap.items.length}>清除记录</button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <ul bind:this={list}>
      {#each items as it, i (it.index)}
        <li class:on={i === sel} class:current={it.index === snap.index}>
          <button class="row" onclick={() => pick(it)} onmousemove={() => (sel = i)} title={it.path || it.title}>
            <span class="title">{it.title}</span>
            <span class="meta">{it.libTitle}</span>
          </button>
        </li>
      {:else}
        <li class="empty muted">{filter ? "没有匹配的记录。" : "还没有可导航的视图。"}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 10vh;
  }
  .box {
    width: min(760px, calc(100vw - 32px)); max-height: 72vh; display: flex; flex-direction: column;
    background: var(--paper); border-radius: 10px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3); outline: none;
  }
  .head { display: flex; gap: 12px; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--line); }
  .head input { flex: 1; }
  .head strong { white-space: nowrap; }
  ul { list-style: none; margin: 0; padding: 4px; overflow: auto; min-height: 0; }
  li { display: flex; align-items: center; border-radius: 6px; }
  li.on { background: var(--accent-soft); }
  li.current { box-shadow: inset 4px 0 0 var(--accent); }
  .row {
    flex: 1; min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 14px; align-items: baseline;
    text-align: left; border: none; background: none; padding: 7px 10px;
  }
  .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 12px; color: var(--muted); white-space: nowrap; }
  .empty { padding: 18px; text-align: center; }
  .error { color: var(--hl); font-size: 13px; margin: 0; padding: 6px 16px; }
</style>
