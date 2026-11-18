<script>
  // Modal dialog for browsing, filtering, renaming, restoring, and clearing AI chat history.
  import { onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";

  let {
    activeSessionId = null,
    doc = null,
    onselect,
    onrename = null,
    onclearcurrent = null,
    onclose,
  } = $props();

  let filter = $state("");
  let onlyDoc = $state(false);
  let items = $state([]);
  let sel = $state(0);
  let input = $state();
  let list = $state();
  let error = $state("");
  let editingId = $state(null);
  let editingTitle = $state("");
  let editInput = $state();

  async function load() {
    try {
      const byDoc = onlyDoc && doc?.lib && doc?.path;
      items = await api.aiSessionList(byDoc ? doc.lib : null, byDoc ? doc.path : null, 300);
      sel = 0;
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    load();
    input?.focus();
  });

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (it) =>
        (it.title || "").toLowerCase().includes(q) ||
        (it.doc_title || "").toLowerCase().includes(q),
    );
  });

  function ago(t) {
    if (!t) return "";
    const s = Math.max(0, Date.now() / 1000 - t);
    if (s < 60) return "刚刚";
    if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
    if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
    if (s < 86400 * 30) return `${Math.floor(s / 86400)} 天前`;
    return new Date(t * 1000).toLocaleDateString("zh-CN");
  }

  function pick(it) {
    if (editingId === it.id) return;
    onselect?.(it.id);
    onclose?.();
  }

  function startRename(it, e) {
    e?.stopPropagation();
    editingId = it.id;
    editingTitle = it.title || "";
    tick().then(() => {
      editInput?.focus();
      editInput?.select();
    });
  }

  async function commitRename(it) {
    const next = editingTitle.trim();
    editingId = null;
    if (!next || next === it.title) return;
    try {
      await api.aiSessionRename(it.id, next);
      it.title = next;
      onrename?.(it.id, next);
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(it, e) {
    e?.stopPropagation();
    try {
      await api.aiSessionDelete(it.id);
      items = items.filter((x) => x.id !== it.id);
      if (it.id === activeSessionId) onclearcurrent?.();
    } catch (err) {
      error = String(err);
    }
  }

  async function clearAll() {
    if (
      !(await ask("确定清空所有已保存的 AI 会话历史？\n（不会影响任何知识库文档本身）", {
        title: "清空会话历史",
        kind: "warning",
        okLabel: "清空",
        cancelLabel: "取消",
      }))
    ) {
      return;
    }
    try {
      await api.aiSessionClear();
      items = [];
      onclearcurrent?.();
    } catch (e) {
      error = String(e);
    }
  }

  function key(e) {
    if (editingId != null) return;
    if (e.key === "Escape") onclose?.();
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = Math.min(shown.length - 1, sel + 1);
      list?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, sel - 1);
      list?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && shown[sel]) {
      e.preventDefault();
      pick(shown[sel]);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="AI 会话历史" tabindex="-1" onkeydown={key}>
    <div class="head">
      <strong>AI 会话历史</strong>
      <input type="search" placeholder="按对话标题或关联文档筛选" bind:value={filter} bind:this={input} />
      {#if doc?.lib && doc?.path}
        <label class="chk">
          <input type="checkbox" bind:checked={onlyDoc} onchange={load} />
          <span>仅当前文档</span>
        </label>
      {/if}
      <button class="link" onclick={clearAll} disabled={!items.length}>清空历史</button>
      <button class="link" onclick={() => onclose?.()}>关闭</button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <ul bind:this={list}>
      {#each shown as it, i (it.id)}
        <li class:on={i === sel} class:current={it.id === activeSessionId} class:row-current={it.id === activeSessionId}>
          {#if editingId === it.id}
            <form class="rename-form" onsubmit={(e) => { e.preventDefault(); commitRename(it); }}>
              <input
                type="text"
                bind:this={editInput}
                bind:value={editingTitle}
                onblur={() => commitRename(it)}
                onkeydown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    editingId = null;
                  }
                }} />
              <button type="submit" class="link">保存</button>
            </form>
          {:else}
            <button class="row" onclick={() => pick(it)} onmousemove={() => (sel = i)}>
              <span class="title">{it.title}</span>
              <span class="meta">{it.doc_title || ""}</span>
              <span class="when">{ago(it.updated)}</span>
            </button>
            <button class="act" title="重命名此会话" aria-label="重命名" onclick={(e) => startRename(it, e)}>改名</button>
            <button class="x" title="删除此条历史" aria-label="删除" onclick={(e) => remove(it, e)}>×</button>
          {/if}
        </li>
      {:else}
        <li class="empty muted">{filter ? "没有匹配的会话记录。" : "暂无保存的会话记录。"}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.25);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 10vh;
  }
  .box {
    width: min(760px, calc(100vw - 32px)); max-height: 72vh; display: flex; flex-direction: column;
    background: var(--paper); border-radius: 10px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3); outline: none;
  }
  .head { display: flex; gap: 10px; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--line); }
  .head input[type="search"] { flex: 1; min-width: 140px; }
  .chk { display: inline-flex; align-items: center; gap: 4px; font-size: 13px; white-space: nowrap; }
  .error { color: var(--hl); padding: 8px 16px 0; margin: 0; font-size: 13px; }
  ul { list-style: none; margin: 0; padding: 6px; overflow: auto; }
  li { display: flex; align-items: center; border-radius: 6px; padding-right: 4px; }
  li.on { background: var(--accent-soft); }
  /* the session in use is .row-current (app.css); its title no longer needs
     a colour of its own */
  .row {
    flex: 1; min-width: 0; display: flex; gap: 10px; align-items: baseline; text-align: left;
    border: none; background: none; padding: 7px 10px;
  }
  .title { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 48%; }
  .meta { color: var(--muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .when { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .act { border: none; background: none; color: var(--muted); font-size: 12px; padding: 4px 6px; border-radius: 4px; }
  .act:hover { color: var(--accent); background: var(--panel); }
  .x { border: none; background: none; color: var(--muted); font-size: 16px; padding: 2px 8px; border-radius: 4px; }
  .x:hover { color: var(--hl); background: var(--panel); }
  .rename-form { flex: 1; display: flex; gap: 8px; align-items: center; padding: 4px 10px; }
  .rename-form input { flex: 1; font-size: 14px; padding: 3px 8px; }
  .empty { padding: 18px 12px; font-size: 14px; }
</style>
