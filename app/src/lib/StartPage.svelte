<script>
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte.js";
  import { formatShortcut } from "./shortcuts.js";

  let { state: start, actions = {} } = $props();
  let recent = $state([]);
  let recentError = $state("");
  const libraries = $derived(start.libraries);
  const canSearch = $derived(libraries.some((library) => library.index_ready && !library.error));
  const searchKey = $derived(formatShortcut("search", settings.shortcuts));
  const quickKey = $derived(formatShortcut("quick", settings.shortcuts));

  onMount(() => {
    api.recentList("", 6).then((items) => { recent = items; }).catch((e) => { recentError = String(e); });
  });

  function indexLabel(library) {
    if (library.error) return "资料不可用";
    if (library.building) return "正在建立索引";
    if (library.index_ready) return "可全文搜索";
    return "尚未建立索引";
  }
</script>

<div class="start-page">
  <div class="start-inner">
    <header>
      <h1>Smart Library</h1>
      {#if libraries.length}
        <div class="buttons">
          <button type="button" class="primary" onclick={() => actions.onsearch?.()} title={searchKey || undefined}><Icon name="search" size={16} />全文搜索</button>
          <button type="button" onclick={() => actions.onquick?.()} title={quickKey || undefined}>标题快搜</button>
          <button type="button" onclick={() => actions.onbind?.("folder")}>添加本地资料…</button>
        </div>
      {:else}
        <p class="muted">添加资料后即可浏览。全文索引在后台自动建立。</p>
        <div class="buttons">
          <button class="primary" type="button" onclick={() => actions.onbind?.("folder")}><Icon name="plus" size={16} />添加本地资料…</button>
          <button type="button" onclick={() => actions.onnewnote?.()}>新建笔记</button>
          <button type="button" onclick={() => actions.onbind?.("import")}>导入数据包…</button>
        </div>
      {/if}
    </header>

    {#if libraries.length}
      <div class="overview">
        <section aria-label="继续阅读">
          <div class="section-head"><h2>继续阅读</h2><button type="button" class="link" onclick={() => actions.onrecent?.()}>最近访问</button></div>
          {#each recent as item (`${item.lib}|${item.path}`)}
            <button class="row" type="button" onclick={() => actions.onopenrecent?.(item)}>
              <span>{item.title}</span><span class="muted meta">{item.lib_title}{item.lb ? ` · ${item.lb}` : ""}</span>
            </button>
          {:else}
            <p class="muted hint">{recentError || "打开资料后，可以从这里继续阅读。"}</p>
          {/each}
        </section>
        <section aria-label="知识库">
          <div class="section-head"><h2>知识库</h2><button type="button" class="link" onclick={() => actions.onbind?.("")}>添加知识库…</button></div>
          {#each libraries as library (library.id)}
            <div class="row library">
              <button class="link" type="button" onclick={() => actions.onopenlib?.(library)} title={library.title}>{library.title}</button>
              {#if !library.error && !library.building && !library.index_ready}
                <button class="link meta" type="button" onclick={() => actions.onindex?.(library.id)}>建立索引</button>
              {:else}
                <span class="muted meta">{indexLabel(library)}</span>
              {/if}
            </div>
          {/each}
        </section>
      </div>
      {#if !canSearch && libraries.some((library) => library.building)}
        <p class="muted" role="status">索引建立期间可以浏览资料或使用标题快搜。</p>
      {/if}
    {/if}

    <div class="buttons utility" aria-label="创建与导入">
      {#if libraries.length}<button type="button" onclick={() => actions.onnewnote?.()}>新建笔记</button>{/if}
      <button type="button" onclick={() => actions.onnewmindmap?.()}>新建脑图</button>
      <button type="button" onclick={() => actions.onnewwhiteboard?.()}>新建白板</button>
      <button type="button" onclick={() => actions.ondiary?.()}>今日记</button>
      {#if !libraries.length}<button type="button" class="link" onclick={() => actions.onbind?.("")}>其他知识库来源…</button>{/if}
    </div>

    <details>
      <summary>AI 与 Agent 工具</summary>
      <div class="buttons">
        <button type="button" onclick={() => actions.onai?.()}>{start.aiConfigured ? "打开 AI 助手" : "配置 AI 助手…"}</button>
        <button type="button" onclick={() => actions.onsettings?.("mcp")}>{start.mcpRunning ? "查看本地 Agent 接入" : "配置本地 Agent（MCP）…"}</button>
      </div>
    </details>
  </div>
</div>

<style>
  .start-page { height: 100%; overflow: auto; background: var(--bg); color: var(--ink); padding: 30px clamp(16px, 4%, 34px); }
  .start-inner { max-width: 900px; margin: 0 auto; display: grid; gap: 18px; }
  header { display: grid; gap: 10px; padding-bottom: 16px; border-bottom: 1px solid var(--line); }
  h1 { margin: 0; font-size: 25px; }
  h2 { margin: 0; font-size: 16px; }
  p { margin: 0; }
  .buttons { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .buttons button { display: inline-flex; gap: 6px; align-items: center; }
  .overview { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(260px, 100%), 1fr)); gap: 12px; align-items: start; }
  section { background: var(--paper); border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
  .section-head { display: flex; align-items: center; justify-content: space-between; gap: 6px; padding: 10px 12px; border-bottom: 1px solid var(--line); }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px; width: 100%; padding: 7px 12px; text-align: left; border: 0; border-radius: 0; background: none; }
  .row + .row { border-top: 1px solid var(--line); }
  button.row:hover { background: var(--accent-soft); }
  .row > :first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library button.link { padding: 0; text-align: left; }
  .meta { font-size: 12px; flex: none; }
  .hint { padding: 10px 12px; }
  details { color: var(--muted); }
  details summary { cursor: pointer; width: fit-content; }
  details .buttons { margin-top: 8px; }
</style>
