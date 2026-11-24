<script>
  // Interactive Start Page rendered in the empty Dockview workspace.
  // Gives newcomers immediate visibility into libraries, AI assistant / built-in
  // Agent, local MCP service, and creation tools without blocking modal wizards.
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte.js";
  import { formatShortcut, formatCombo } from "./shortcuts.js";

  let { state: st, actions = {} } = $props();

  const HIDE_KEY = "sl.start.hideGuide";
  let hideGuide = $state(false);
  try {
    hideGuide = localStorage.getItem(HIDE_KEY) === "1";
  } catch {}
  function toggleGuide(v) {
    hideGuide = v;
    try {
      localStorage.setItem(HIDE_KEY, v ? "1" : "0");
    } catch {}
  }

  const hasLibs = $derived((st?.libraryCount ?? 0) > 0);
  const aiReady = $derived(!!st?.aiConfigured);
  const mcpRunning = $derived(!!st?.mcpRunning);
  const step1Done = $derived(hasLibs);
  const step2Done = $derived(aiReady || mcpRunning);
  const doneCount = $derived((step1Done ? 1 : 0) + (step2Done ? 1 : 0));

  const searchKey = $derived(formatShortcut("search", settings.shortcuts) || formatCombo("Ctrl+H"));
  const aiKey = $derived(formatShortcut("ai", settings.shortcuts) || formatCombo("Ctrl+Shift+A"));
</script>

<div class="start-page">
  <div class="start-inner">
    <header class="hero">
      <div class="hero-text">
        <h1>Smart Library</h1>
        <p class="muted">本地知识库阅读、创作与 AI / Agent 协同工作台</p>
      </div>
      <div class="hero-actions">
        <button type="button" class:primary={!hasLibs} onclick={() => actions.onbind?.("")}>
          <Icon name="plus" size={15} /><span>添加知识库</span>
        </button>
        <button type="button" onclick={() => actions.onsearch?.()}>
          <Icon name="search" size={15} /><span>全文搜索</span>
        </button>
        <button type="button" onclick={() => actions.onquick?.()}>
          <Icon name="plane" size={15} /><span>标题快搜</span>
        </button>
      </div>
    </header>

    <section class="cards" aria-label="核心功能入口">
      <article class="card">
        <div class="card-head">
          <span class="card-icon"><Icon name="library" size={18} /></span>
          <h2>建立知识库</h2>
          <span class="badge" class:ok={hasLibs}>
            {hasLibs ? `已添加 ${st.libraryCount} 个` : "第一步"}
          </span>
        </div>
        <p class="muted card-desc">
          挂载本地文件夹、CBETA 电子佛典、ZIM 百科、远程 Git 仓库，或创建内部笔记库统一检索阅读。
        </p>
        <div class="card-actions">
          <button type="button" class:primary={!hasLibs} onclick={() => actions.onbind?.("")}>添加知识库…</button>
          <button type="button" onclick={() => actions.onbind?.("import")}>导入数据包…</button>
        </div>
      </article>

      <article class="card accent-card">
        <div class="card-head">
          <span class="card-icon"><Icon name="sparkles" size={18} /></span>
          <h2>AI 助手 &amp; 知识库 Agent</h2>
          <span class="badge" class:ok={aiReady} class:warn={!aiReady}>
            {aiReady ? "已就绪" : "待配置"}
          </span>
        </div>
        <p class="muted card-desc">
          随时伴读问答与 <code>@</code> 跨文档引用；内置 Agent 可主动搜索知识库、整理标签并直接生成笔记、脑图与白板。
        </p>
        <div class="card-actions">
          <button type="button" class="primary" onclick={() => actions.onai?.()}>
            {aiReady ? "打开 AI 助手" : "1 分钟快速配置"}
          </button>
          <button type="button" onclick={() => actions.onsettings?.("ai")}>模型设置…</button>
        </div>
      </article>

      <article class="card">
        <div class="card-head">
          <span class="card-icon"><Icon name="sliders" size={18} /></span>
          <h2>本地 Agent 接入（MCP）</h2>
          <span class="badge" class:ok={mcpRunning}>
            {mcpRunning ? "运行中" : "未开启"}
          </span>
        </div>
        <p class="muted card-desc">
          开启本地 MCP 服务，让 Claude、Cursor、Codex 等外部 AI Agent 安全检索与读写你的知识库，支持一键复制接入配置。
        </p>
        <div class="card-actions">
          <button type="button" onclick={() => actions.onsettings?.("mcp")}>
            {mcpRunning ? "查看接入配置…" : "开启本地 MCP…"}
          </button>
        </div>
      </article>

      <article class="card">
        <div class="card-head">
          <span class="card-icon"><Icon name="file" size={18} /></span>
          <h2>快速开始创作</h2>
          <span class="badge">本地存储</span>
        </div>
        <p class="muted card-desc">
          用 Markdown 记笔记、用脑图梳理脉络、用白板绘制关系图，或一键打开今日记沉淀每日灵感。
        </p>
        <div class="card-actions wrap">
          <button type="button" onclick={() => actions.onnewnote?.()}>新建笔记</button>
          <button type="button" onclick={() => actions.onnewmindmap?.()}>脑图</button>
          <button type="button" onclick={() => actions.onnewwhiteboard?.()}>白板</button>
          <button type="button" onclick={() => actions.ondiary?.()}>今日记</button>
        </div>
      </article>
    </section>

    {#if !hideGuide}
      <section class="guide" aria-label="新手上手清单">
        <div class="guide-head">
          <strong>快速上手（{doneCount}/2 项准备就绪）</strong>
          <button type="button" class="link" onclick={() => toggleGuide(true)}>收起</button>
        </div>
        <ol class="steps">
          <li class:done={step1Done}>
            <span class="check">{step1Done ? "✓" : "1"}</span>
            <span class="step-text">添加或导入第一个知识库，建立本地全文索引</span>
            {#if !step1Done}
              <button type="button" class="link" onclick={() => actions.onbind?.("")}>去添加</button>
            {/if}
          </li>
          <li class:done={step2Done}>
            <span class="check">{step2Done ? "✓" : "2"}</span>
            <span class="step-text">配置内置 AI 大模型（支持 DeepSeek / OpenAI / Claude / 本地 Ollama）或开启本地 MCP 服务</span>
            {#if !step2Done}
              <button type="button" class="link" onclick={() => actions.onai?.()}>去配置 AI</button>
              <button type="button" class="link" onclick={() => actions.onsettings?.("mcp")}>配置 MCP</button>
            {/if}
          </li>
        </ol>
      </section>
    {:else}
      <div class="guide-collapsed">
        <button type="button" class="link" onclick={() => toggleGuide(false)}>显示快速上手清单</button>
      </div>
    {/if}

    <footer class="tips muted">
      <span>快捷键：<code>{searchKey}</code> 全文搜索 · 双击 <code>Shift</code> 标题快搜 · <code>{aiKey}</code> AI 助手</span>
      <span>标签可拖到边缘分栏停靠；右键标签可分栏、浮动或在新窗口打开。</span>
    </footer>
  </div>
</div>

<style>
  .start-page {
    height: 100%; overflow: auto; background: var(--bg); color: var(--ink);
    padding: 32px 28px 24px;
  }
  .start-inner {
    max-width: 900px; margin: 0 auto; display: flex; flex-direction: column; gap: 20px;
  }
  .hero {
    display: flex; align-items: flex-end; justify-content: space-between; gap: 16px; flex-wrap: wrap;
    padding-bottom: 14px; border-bottom: 1px solid var(--line);
  }
  .hero h1 { margin: 0 0 4px; font-size: 24px; font-weight: 700; letter-spacing: 0.01em; color: var(--ink); }
  .hero p { margin: 0; font-size: 14px; }
  .hero-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .hero-actions button {
    display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; font-size: 13.5px;
  }

  .cards {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 14px;
  }
  .card {
    background: var(--paper); border: 1px solid var(--line); border-radius: 10px;
    padding: 16px 18px; display: flex; flex-direction: column; gap: 10px;
  }
  .card.accent-card {
    border-color: color-mix(in srgb, var(--accent) 42%, var(--line));
  }
  .card-head { display: flex; align-items: center; gap: 8px; }
  .card-icon {
    display: inline-flex; align-items: center; justify-content: center;
    width: 30px; height: 30px; border-radius: 8px;
    background: var(--accent-soft); color: var(--accent); flex: none;
  }
  .card-head h2 { margin: 0; font-size: 15.5px; font-weight: 600; flex: 1; min-width: 0; }
  .badge {
    font-size: 11.5px; padding: 2px 8px; border-radius: 999px; white-space: nowrap;
    background: var(--panel); color: var(--muted); border: 1px solid var(--line);
  }
  .badge.ok {
    background: color-mix(in srgb, var(--note) 14%, var(--paper));
    color: var(--note); border-color: color-mix(in srgb, var(--note) 40%, var(--line));
  }
  .badge.warn {
    background: var(--accent-soft); color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
  }
  .card-desc { margin: 0; font-size: 13px; line-height: 1.6; flex: 1; }
  .card-desc code {
    font-size: 12px; padding: 0 4px; border-radius: 4px; background: var(--panel); color: var(--ink);
  }
  .card-actions { display: flex; align-items: center; gap: 8px; padding-top: 2px; }
  .card-actions.wrap { flex-wrap: wrap; }
  .card-actions button { font-size: 13px; padding: 5px 11px; }

  .guide {
    background: var(--paper); border: 1px solid var(--line); border-radius: 10px;
    padding: 13px 16px; display: flex; flex-direction: column; gap: 8px;
  }
  .guide-head { display: flex; align-items: center; justify-content: space-between; font-size: 13.5px; }
  .steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .steps li {
    display: flex; align-items: center; gap: 8px; font-size: 13px; flex-wrap: wrap;
    padding: 4px 6px; border-radius: 6px;
  }
  .steps li.done .step-text { color: var(--muted); text-decoration: line-through; }
  .check {
    width: 20px; height: 20px; border-radius: 50%; flex: none;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 11px; font-weight: 600;
    background: var(--panel); color: var(--muted); border: 1px solid var(--line);
  }
  .steps li.done .check { background: var(--note); color: var(--paper); border-color: var(--note); }
  .step-text { flex: 1; min-width: 180px; }
  .guide-collapsed { display: flex; justify-content: flex-end; font-size: 12.5px; }

  .tips {
    display: flex; flex-direction: column; gap: 4px; font-size: 12.5px;
    padding-top: 8px; border-top: 1px dashed var(--line);
  }
  .tips code {
    font-size: 12px; padding: 0 4px; border-radius: 4px; background: var(--panel); color: var(--ink);
  }
</style>
