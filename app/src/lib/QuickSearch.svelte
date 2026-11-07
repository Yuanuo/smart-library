<script module>
  // kept between openings, like IDEA's Search Everywhere
  let lastQuery = "";
  let lastLib = null;
  let lastQuote = null;

  // `#` commands (CBETA): shown when the query is `#` or not a valid command
  const COMMANDS = [
    ["#T01n0001_p0001a01", "转到 T0001 的 0001a01 行"],
    ["#T0001_p0001a01", "转到 T0001 的 0001a01 行"],
    ["#T01, no. 1, p. 1a1", "转到 T0001 的 0001a01 行（CBETA 引用格式）"],
    ["#T01n0001_1、#T01n0001_001", "转到 T0001 的第 1 卷"],
    ["#T0001_1、#T0001_001", "转到 T0001 的第 1 卷"],
    ["#T0001", "转到 T0001"],
  ];
</script>

<script>
  // Title quick search (design-spec 8.7): a popup opened by pressing Shift
  // twice. Searches the titles of books, chapters, juan and articles as you
  // type; the tabs limit it to one knowledge library.
  import { onMount, tick } from "svelte";
  import * as api from "./api.js";

  // `scope`: { lib, label, within } when started from a node of the tree
  let { libraries, onopen, onclose, scope = null } = $props();

  const KIND = { book: "经", chapter: "目", juan: "卷", article: "文", cite: "引", cite_missing: "引" };
  const KIND_TITLE = {
    book: "经",
    chapter: "目次（序、品、分、经…）",
    juan: "卷",
    article: "文章",
    cite: "CBETA 引用",
    cite_missing: "CBETA 引用",
  };

  let query = $state(lastQuery);
  let quote = $state(lastQuote); // quote of a pasted citation, highlighted when opened
  // svelte-ignore state_referenced_locally
  let lib = $state(scope ? scope.lib : libraries.some((l) => l.id === lastLib) ? lastLib : null);
  let result = $state(null);
  let error = $state("");
  let loading = $state(false);
  let selected = $state(0);
  let input = $state();
  let list = $state();
  let seq = 0;

  const isCommand = $derived(query.trim().startsWith("#"));
  const tabs = $derived(scope ? [] : [{ id: null, title: "全部" }, ...libraries.map((l) => ({ id: l.id, title: l.title }))]);
  const libTitle = $derived(Object.fromEntries(libraries.map((l) => [l.id, l.title])));
  const counts = $derived(Object.fromEntries((result?.libraries ?? []).map((c) => [c.id, c.count])));
  const allCount = $derived((result?.libraries ?? []).reduce((n, c) => n + c.count, 0));

  onMount(() => {
    input.focus();
    input.select();
    run();
  });

  async function run() {
    const my = ++seq;
    if (!query.trim().startsWith("#")) quote = null;
    if (!scope) {
      lastQuery = query;
      lastLib = lib;
      lastQuote = quote;
    }
    loading = true;
    try {
      const r = await api.quickSearch(query, lib, 200, quote, scope?.within ?? null);
      if (my !== seq) return;
      result = r;
      error = "";
      selected = 0;
      list?.scrollTo(0, 0);
    } catch (e) {
      if (my === seq) error = String(e);
    } finally {
      if (my === seq) loading = false;
    }
  }

  // pasted CBETA citation text -> its `#` command (the quote is kept apart)
  async function paste(e) {
    const text = e.clipboardData?.getData("text") ?? "";
    if (!text) return;
    try {
      const c = await api.citationCommand(text);
      if (c && query.includes(text.trim())) {
        query = c.command;
        quote = c.quote;
        run();
      }
    } catch {}
  }

  function setLib(id) {
    lib = id;
    run();
    input.focus();
  }

  async function move(delta) {
    const n = result?.items.length ?? 0;
    if (!n) return;
    selected = Math.max(0, Math.min(n - 1, selected + delta));
    await tick();
    list?.querySelector(`[data-i="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function choose(item) {
    if (!item || item.kind === "cite_missing") return;
    onclose();
    onopen(item);
  }

  function keydown(e) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "PageDown":
        e.preventDefault();
        move(10);
        break;
      case "PageUp":
        e.preventDefault();
        move(-10);
        break;
      case "Enter":
        e.preventDefault();
        choose(result?.items[selected]);
        break;
      case "Escape":
        e.preventDefault();
        onclose();
        break;
      case "Tab": {
        // next / previous knowledge library
        e.preventDefault();
        if (!tabs.length) break;
        const i = tabs.findIndex((t) => t.id === lib);
        const next = tabs[(i + (e.shiftKey ? tabs.length - 1 : 1)) % tabs.length];
        setLib(next.id);
        break;
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="backdrop" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-label="标题快搜" aria-modal="true">
    <div class="tabs" role="tablist" aria-label="知识库">
      {#if scope}<span class="scopechip" title="从目录树的这个节点搜索">范围：<b>{scope.label}</b></span>{/if}
      {#each tabs as t (t.id)}
        <button role="tab" class:active={t.id === lib} aria-selected={t.id === lib} tabindex="-1"
          onclick={() => setLib(t.id)}>
          {t.title}
          {#if result}<span class="count">{t.id === null ? allCount : (counts[t.id] ?? 0)}</span>{/if}
        </button>
      {/each}
      <span class="help" title={"输入经名、经号、品名、作译者；空格分隔多个关键词，简繁通用。\n输入 # 转到 CBETA 的经、卷或行（如 #T0001_001、#T01n0001_p0001a01），也可以直接粘贴 CBETA 的引用文字。\n↑↓ 选择，Enter 打开，Tab 切换知识库，Esc 关闭。"}>?</span>
    </div>
    <div class="field">
      <input bind:this={input} bind:value={query} oninput={run} onpaste={paste} onkeydown={keydown}
        placeholder="经名、经号、品名、作译者（空格分隔关键词）" aria-label="快搜" spellcheck="false" />
      <span class="stat">
        {#if loading && !result}准备标题…
        {:else if result}{result.total > result.items.length ? `显示 ${result.items.length} / ${result.total} 项` : `${result.total} 项`}，{result.millis.toFixed(0)} ms
        {/if}
      </span>
    </div>
    {#if error}
      <p class="note error">{error}</p>
    {:else if !libraries.length}
      <p class="note">还没有绑定知识库。</p>
    {:else if isCommand && !result?.items.length}
      <div class="note">
        <p>转到 CBETA 的经、卷或行，请使用以下格式（也可以直接粘贴 CBETA 的引用文字）：</p>
        <table class="cmds">
          <tbody>{#each COMMANDS as [c, d]}<tr><td><code>{c}</code></td><td>{d}</td></tr>{/each}</tbody>
        </table>
      </div>
    {:else if result && !result.items.length}
      <p class="note">没有找到。</p>
    {/if}
    <ul class="list" bind:this={list} role="listbox" aria-label="结果">
      {#each result?.items ?? [] as item, i (i)}
        <li data-i={i} role="option" aria-selected={i === selected} class:sel={i === selected}
          onmousemove={() => (selected = i)} onclick={() => choose(item)}>
          <span class="kind k-{item.kind}" title={KIND_TITLE[item.kind]}>{KIND[item.kind]}</span>
          <span class="main">
            {#if item.kind === "cite"}<span class="go">转到 »</span>{/if}
            <span class="title" class:missing={item.kind === "cite_missing"}>{item.title}</span>
            {#if item.kind === "cite"}<span class="ctx">{item.lb}</span>{/if}
            {#if item.context}<span class="ctx">{item.context}</span>{/if}
            {#if item.author}<span class="ctx">{item.author}</span>{/if}
          </span>
          {#if lib === null && libraries.length > 1}<span class="lib">{libTitle[item.lib]}</span>{/if}
        </li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 1000; background: rgba(0, 0, 0, 0.18);
    display: flex; justify-content: center; align-items: flex-start; padding-top: 8vh;
  }
  .dialog {
    width: min(920px, calc(100vw - 32px)); max-height: 78vh; display: flex; flex-direction: column;
    background: var(--paper); border: 1px solid var(--line); border-radius: 8px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25); overflow: hidden;
  }
  .tabs, .field { flex: none; }
  .tabs {
    display: flex; align-items: center; gap: 2px; padding: 6px 8px 0; background: var(--panel);
    border-bottom: 1px solid var(--line); overflow-x: auto;
  }
  .tabs button {
    border: none; background: none; padding: 6px 10px; border-radius: 6px 6px 0 0; white-space: nowrap;
    font-size: 13px; color: var(--muted); border-bottom: 2px solid transparent;
  }
  .tabs button:hover { color: var(--ink); }
  .scopechip { padding: 6px 4px; color: var(--muted); }
  .scopechip b { color: var(--accent); }
  .tabs button.active { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }
  .count { font-size: 11px; color: var(--muted); margin-left: 3px; font-weight: 400; }
  .help {
    margin-left: auto; width: 20px; height: 20px; flex: none; border-radius: 50%; border: 1px solid var(--muted);
    color: var(--muted); font-size: 12px; display: grid; place-items: center; cursor: help;
  }
  .field { display: flex; align-items: center; gap: 12px; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .field input { flex: 1; font-size: 16px; border: none; background: none; outline: none; padding: 4px 0; color: var(--ink); }
  .stat { font-size: 12px; color: var(--muted); white-space: nowrap; }
  .note { padding: 8px 14px; margin: 0; font-size: 14px; color: var(--muted); }
  .note.error { color: var(--hl); }
  .note p { margin: 0 0 6px; }
  .cmds { border-collapse: collapse; color: var(--ink); }
  .cmds td { padding: 3px 16px 3px 0; border-bottom: 1px dashed var(--line); }
  .cmds code { font-family: Consolas, monospace; font-size: 13px; }
  .list { list-style: none; margin: 0; padding: 4px 0; overflow-y: auto; flex: 1; min-height: 0; }
  .list li {
    display: flex; align-items: center; gap: 8px; padding: 4px 12px; cursor: pointer; min-height: 28px;
  }
  .list li.sel { background: var(--accent-soft); }
  .kind {
    flex: none; width: 20px; height: 20px; border-radius: 4px; display: grid; place-items: center;
    font-size: 12px; color: var(--paper); background: var(--muted);
  }
  .k-book, .k-article, .k-cite { background: var(--accent); }
  .go { color: var(--accent); font-weight: 600; margin-right: 4px; }
  .missing { color: var(--hl); }
  .main { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .title { color: var(--ink); }
  .ctx { color: var(--muted); font-size: 13px; }
  .ctx::before { content: " / "; }
  .lib { flex: none; font-size: 12px; color: var(--muted); max-width: 30%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
