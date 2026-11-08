<script>
  // Add the selected text to a note: a new one (at a place in an internal
  // library) or an existing one. The text goes in as a quote with a link back
  // to its source; a remark of your own can follow it.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import { getLast } from "./clipTarget.js";

  let { text, vol, onok, onclose } = $props();

  const last = getLast();
  let mode = $state(last ? "append" : "new");
  let where = $state("");
  let title = $state(`摘录：${vol.title}`);
  let find = $state(last ? last.title : ""); // append: what is typed to find the note
  let target = $state(last); // { lib, id, title }
  let comment = $state("");
  let hints = $state([]);
  let hi = $state(-1);
  let busy = $state(false);
  let error = $state("");
  let timer = null;
  let skip = false;
  let first = $state();

  onMount(() => first?.focus());

  function typed(query) {
    if (skip) {
      skip = false;
      return;
    }
    clearTimeout(timer);
    timer = setTimeout(async () => {
      try {
        const all = await api.noteTargets(query);
        hints = (mode === "append" ? all.filter((h) => h.id) : all).slice(0, 8);
        hi = -1;
      } catch {
        hints = [];
      }
    }, 120);
  }

  function pick(h) {
    skip = true;
    if (mode === "append") {
      target = { lib: h.lib, id: h.id, title: h.path.split("/").pop() };
      find = h.path;
    } else where = h.path;
    hints = [];
    hi = -1;
  }

  function key(e) {
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

  function setMode(m) {
    mode = m;
    hints = [];
  }

  const ready = $derived(mode === "new" ? !!title.trim() : !!target);

  async function submit(e) {
    e.preventDefault();
    if (!ready || busy) return;
    busy = true;
    error = "";
    try {
      await onok({ mode, where, title: title.trim(), target, comment });
      onclose();
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <form class="dialog" role="dialog" aria-modal="true" aria-label="加入笔记" onsubmit={submit}
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>加入笔记</h2>
    <p class="quote">{text.length > 160 ? text.slice(0, 160) + "…" : text}</p>
    <p class="src muted">来源：《{vol.title}》{vol.author ? " " + vol.author : ""}，笔记里的来源链接可以回到原文位置。</p>

    <div class="seg" role="group" aria-label="方式">
      <button type="button" class:on={mode === "append"} onclick={() => setMode("append")}>追加到已有笔记</button>
      <button type="button" class:on={mode === "new"} onclick={() => setMode("new")}>新建笔记</button>
    </div>

    {#if mode === "append"}
      <label>笔记
        <div class="where">
          <!-- svelte-ignore a11y_autofocus -->
          <input type="text" bind:value={find} bind:this={first} autocomplete="off" placeholder="输入笔记名称或路径查找"
            oninput={() => { target = null; typed(find); }} onfocus={() => typed(find)} onkeydown={key} />
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
      <p class="hint muted">{target ? `将追加到「${target.title}」的末尾。` : "从下拉列表里选一篇笔记。"}（正在编辑的笔记会追加到编辑框里。）</p>
    {:else}
      <label>位置
        <div class="where">
          <input type="text" bind:value={where} bind:this={first} autocomplete="off"
            placeholder="知识库/文件夹/…（留空：默认知识库的顶层）"
            oninput={() => typed(where)} onfocus={() => typed(where)} onkeydown={key} />
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
      <label>标题<input type="text" bind:value={title} /></label>
    {/if}

    <label>我的批注（可不填）<textarea bind:value={comment} rows="2"></textarea></label>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button class="primary" disabled={!ready || busy}>{mode === "new" ? "创建并加入" : "追加"}</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(560px, calc(100vw - 32px)); background: var(--paper); border-radius: 10px;
    padding: 18px 22px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 8px; font-size: 18px; color: var(--accent); }
  .quote {
    margin: 0; padding: 6px 12px; border-left: 3px solid var(--accent); background: var(--bg);
    max-height: 7em; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; border-radius: 0 6px 6px 0;
  }
  .src { margin: 4px 0 10px; font-size: 12px; }
  .seg { display: flex; margin-bottom: 4px; }
  .seg button { border-radius: 0; margin-left: -1px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 6px 6px 0; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  label { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; }
  textarea { resize: vertical; font: inherit; }
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
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>
