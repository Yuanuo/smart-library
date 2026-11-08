<script>
  // Add a note: where (a path in an internal library, with suggestions from
  // the existing notes; folders that do not exist are made) and its title.
  import { onMount } from "svelte";
  import * as api from "./api.js";

  let { position = "", defaultLib = null, onok, onclose } = $props();
  let where = $state(position);
  let title = $state("");
  let kind = $state("md"); // md | mindmap
  let titleInput = $state();
  let whereInput = $state();
  let hints = $state([]);
  let hi = $state(-1);
  let busy = $state(false);
  let error = $state("");
  let timer = null;
  let skip = false; // the next input change came from picking a hint

  onMount(() => titleInput?.focus());

  function typed() {
    if (skip) {
      skip = false;
      return;
    }
    clearTimeout(timer);
    timer = setTimeout(async () => {
      try {
        hints = (await api.noteTargets(where)).slice(0, 8);
        hi = -1;
      } catch {
        hints = [];
      }
    }, 120);
  }

  function pick(h) {
    skip = true;
    where = h.path;
    hints = [];
    hi = -1;
    titleInput?.focus();
  }

  function whereKey(e) {
    if (!hints.length) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      hi = (hi + 1) % hints.length;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      hi = (hi - 1 + hints.length) % hints.length;
    } else if (e.key === "Enter" && hi >= 0) {
      e.preventDefault();
      pick(hints[hi]);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      hints = [];
    }
  }

  async function submit(e) {
    e.preventDefault();
    if (!title.trim() || busy) return;
    busy = true;
    error = "";
    try {
      await onok(where, title.trim(), kind);
      onclose();
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <form class="dialog" role="dialog" aria-modal="true" aria-label="新建笔记" onsubmit={submit}
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>新建笔记</h2>
    <label>位置
      <div class="where">
        <input type="text" bind:value={where} bind:this={whereInput} oninput={typed} onkeydown={whereKey}
          onfocus={typed} placeholder="知识库/文件夹/…（留空：默认知识库的顶层）" autocomplete="off" />
        {#if hints.length}
          <ul class="hints" role="listbox">
            {#each hints as h, i}
              <li role="option" aria-selected={i === hi}>
                <button type="button" class:on={i === hi} onmousedown={(e) => (e.preventDefault(), pick(h))}>{h.path}</button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </label>
    <p class="hint muted">用 / 分隔层级。不存在的层级会在点「创建」时一起建好。</p>
    <label>标题<input type="text" bind:value={title} bind:this={titleInput} /></label>
    <div class="kinds" role="group" aria-label="类型">
      <label class="radio"><input type="radio" bind:group={kind} value="md" /> 文档笔记</label>
      <label class="radio"><input type="radio" bind:group={kind} value="mindmap" /> 脑图</label>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button class="primary" disabled={!title.trim() || busy}>创建</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(520px, calc(100vw - 32px)); background: var(--paper); border-radius: 10px;
    padding: 18px 22px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 10px; font-size: 18px; color: var(--accent); }
  label { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; }
  .where { position: relative; }
  .where input { width: 100%; }
  .hints {
    position: absolute; left: 0; right: 0; top: 100%; z-index: 5; margin: 2px 0 0; padding: 2px; list-style: none;
    background: var(--paper); border: 1px solid var(--line); border-radius: 6px; box-shadow: 0 6px 20px rgba(0, 0, 0, 0.2);
    max-height: 220px; overflow: auto;
  }
  .hints button {
    display: block; width: 100%; text-align: left; border: none; background: none; padding: 4px 8px;
    border-radius: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .hints button:hover, .hints button.on { background: var(--accent-soft); }
  .hint { margin: 4px 0 0; font-size: 12px; }
  .kinds { display: flex; gap: 18px; margin-top: 10px; }
  .radio { flex-direction: row; align-items: center; gap: 6px; margin: 0; }
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>
