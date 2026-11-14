<script>
  import * as api from "./api.js";

  let { text = "", onclose } = $props();

  let pinyin = $state("");
  let loading = $state(true);
  let error = $state("");
  let copied = $state(false);
  let copyTimer = null;

  const charCount = $derived([...text].length);

  $effect(() => {
    const src = text;
    if (!src) {
      pinyin = "";
      loading = false;
      return;
    }
    loading = true;
    error = "";
    api.toPinyin(src)
      .then((res) => {
        if (text === src) pinyin = res;
      })
      .catch((e) => {
        if (text === src) error = String(e);
      })
      .finally(() => {
        if (text === src) loading = false;
      });
  });

  async function copyResult() {
    if (!pinyin) return;
    try {
      await navigator.clipboard.writeText(pinyin);
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 2000);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="查拼音" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose?.()}>
    <header class="dialog-header">
      <h2>查拼音</h2>
      <span class="muted count">共 {charCount.toLocaleString("zh-CN")} 字</span>
    </header>
    <div class="dialog-content">
      {#if error}
        <p class="error">{error}</p>
      {:else if loading}
        <p class="muted">正在转换…</p>
      {:else}
        <div class="py-box" role="region" aria-label="拼音转换结果">{pinyin}</div>
      {/if}
    </div>
    <div class="actions">
      <button type="button" onclick={copyResult} disabled={loading || !pinyin}>
        {copied ? "已复制" : "复制拼音"}
      </button>
      <button type="button" class="primary" onclick={onclose}>关闭</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 960; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(620px, calc(100vw - 32px)); }
  .dialog-header { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: 18px; color: var(--accent); }
  .count { font-size: 12px; }
  .py-box {
    max-height: 52vh; overflow: auto; padding: 12px 14px;
    border: 1px solid var(--line); border-radius: 6px; background: var(--bg);
    font-size: 16px; line-height: 1.85; white-space: pre-wrap; word-break: break-word;
    user-select: text;
  }
  .error { color: var(--hl); margin: 0; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
</style>
