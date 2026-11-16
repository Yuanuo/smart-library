<script>
  import * as api from "./api.js";

  let { text = "", onclose } = $props();

  let chars = $state([]);
  let plain = $state("");
  let annotated = $state("");
  let loading = $state(true);
  let error = $state("");
  let copied = $state("");
  let copyTimer = null;

  const charCount = $derived([...text].length);

  $effect(() => {
    const src = text;
    if (!src) {
      chars = [];
      plain = "";
      annotated = "";
      loading = false;
      return;
    }
    loading = true;
    error = "";
    api.pinyinConvert(src)
      .then((res) => {
        if (text !== src) return;
        chars = res.chars;
        plain = res.plain;
        annotated = res.annotated;
      })
      .catch((e) => {
        if (text === src) error = String(e);
      })
      .finally(() => {
        if (text === src) loading = false;
      });
  });

  async function copy(value, kind) {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      copied = kind;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = ""), 2000);
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
        <div class="py-box" role="region" aria-label="拼音转换结果">{#each chars as c}{#if c.pinyin}<ruby>{c.text}<rt>{c.pinyin}</rt></ruby>{:else}{c.text}{/if}{/each}</div>
      {/if}
    </div>
    <div class="actions">
      <button type="button" onclick={() => copy(plain, "plain")} disabled={loading || !plain}>
        {copied === "plain" ? "已复制" : "复制拼音"}
      </button>
      <button type="button" onclick={() => copy(annotated, "annotated")} disabled={loading || !annotated}>
        {copied === "annotated" ? "已复制" : "复制汉字+拼音"}
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
    max-height: 52vh; overflow: auto; padding: 14px 16px 10px;
    border: 1px solid var(--line); border-radius: 6px; background: var(--bg);
    font-size: 20px; line-height: 2.6; white-space: pre-wrap; word-break: break-word;
    user-select: text;
  }
  .py-box ruby { ruby-position: over; ruby-align: center; margin: 0 2px; letter-spacing: 0.08em; }
  .py-box rt { font-size: 0.8em; line-height: 1.2; color: var(--muted); letter-spacing: 0; }
  .error { color: var(--hl); margin: 0; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
</style>
