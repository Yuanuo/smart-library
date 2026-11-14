<script>
  // A standalone AI chat window popped out from the workspace.
  import { emit } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import AiPanel from "./lib/AiPanel.svelte";

  let { info } = $props();
  const win = getCurrentWebviewWindow();
  const start = (() => info || {})();

  let currentSessionId = $state(start.sessionId || null);
  let currentTitle = $state(start.title || "AI 助手");

  async function dock() {
    await emit("aichat-dock", {
      sessionId: currentSessionId,
      title: currentTitle,
    });
    await win.close();
  }
</script>

<div class="win">
  <div class="bar">
    <span class="title">{currentTitle}</span>
    <button onclick={dock} title="把这个 AI 会话放回主窗口的标签页">停靠回主窗口</button>
  </div>
  <div class="body">
    <AiPanel
      scopeKind="chat"
      initialSessionId={start.sessionId || null}
      onOpenDoc={(req) => emit("popout-open", req)}
      onSessionChange={({ sessionId, title }) => {
        currentSessionId = sessionId;
        if (title) {
          currentTitle = `AI：${title.slice(0, 16)}`;
          win.setTitle(`${currentTitle} — Smart Library`).catch(() => {});
        }
      }} />
  </div>
</div>

<style>
  .win { display: flex; flex-direction: column; height: 100%; }
  .bar {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 14px;
  }
  .title { font-weight: 600; color: var(--accent); }
  .body { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .body > :global(*) { flex: 1; min-height: 0; }
</style>
