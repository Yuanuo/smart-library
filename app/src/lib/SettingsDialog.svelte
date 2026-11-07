<script>
  // Settings: theme, interface zoom and font, reader font, size and line
  // height. Changes apply at once and are kept.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import { settings, update, reset, FONTS } from "./settings.svelte.js";

  let { onclose } = $props();

  // the fonts installed on this computer (from the program, not the browser)
  let cjk = $state([]);
  let other = $state([]);
  onMount(() => {
    api
      .listFonts()
      .then((l) => {
        cjk = l.filter((f) => f.cjk);
        other = l.filter((f) => !f.cjk);
      })
      .catch(() => {});
  });
  const css = (f) => f.replace(/["';]/g, "");
  const shown = (f) => (f.label !== f.family ? `${f.label}（${f.family}）` : f.family);
</script>

{#snippet picker(field, customField, hint)}
  <select value={settings[field]} onchange={(e) => update({ [field]: e.currentTarget.value })}>
    {#each FONTS.filter((f) => f[0] !== "custom") as [v, label]}<option value={v}>{label}</option>{/each}
    {#if cjk.length}
      <optgroup label="本机中文字体">
        {#each cjk as f (f.family)}<option value={"sys:" + f.family} style="font-family: '{css(f.family)}'">{shown(f)}</option>{/each}
      </optgroup>
    {/if}
    {#if other.length}
      <optgroup label="本机其他字体">
        {#each other as f (f.family)}<option value={"sys:" + f.family} style="font-family: '{css(f.family)}'">{shown(f)}</option>{/each}
      </optgroup>
    {/if}
    <option value="custom">手动输入字体名…</option>
  </select>
  {#if settings[field] === "custom"}
    <input type="text" placeholder={hint} value={settings[customField]}
      onchange={(e) => update({ [customField]: e.currentTarget.value })} />
  {/if}
{/snippet}


<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="设置" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>设置</h2>

    <fieldset>
      <legend>外观</legend>
      <div class="row">
        <span>主题</span>
        <div class="seg" role="group" aria-label="主题">
          {#each [["auto", "跟随系统"], ["light", "浅色"], ["dark", "深色"]] as [v, label]}
            <button class:on={settings.theme === v} onclick={() => update({ theme: v })}>{label}</button>
          {/each}
        </div>
      </div>
      <div class="row">
        <span>界面缩放</span>
        <input type="range" min="80" max="150" step="5" value={settings.uiZoom}
          oninput={(e) => update({ uiZoom: Number(e.currentTarget.value) })} />
        <output>{settings.uiZoom}%</output>
      </div>
      <div class="row">
        <span>界面字体</span>
        {@render picker("uiFont", "uiFontCustom", "字体名称，如 Microsoft YaHei")}
      </div>
    </fieldset>

    <fieldset>
      <legend>阅读器</legend>
      <div class="row">
        <span>字体</span>
        {@render picker("readerFont", "readerFontCustom", "字体名称，如 KaiTi")}
      </div>
      <div class="row">
        <span>字号</span>
        <input type="range" min="14" max="34" step="1" value={settings.readerSize}
          oninput={(e) => update({ readerSize: Number(e.currentTarget.value) })} />
        <output>{settings.readerSize} px</output>
      </div>
      <div class="row">
        <span>行距</span>
        <input type="range" min="1.4" max="2.6" step="0.1" value={settings.readerLine}
          oninput={(e) => update({ readerLine: Number(e.currentTarget.value) })} />
        <output>{settings.readerLine}</output>
      </div>
      <p class="sample" aria-label="预览">如是我聞。一時佛在舍衛國祇樹給孤獨園。Sample 0123</p>
    </fieldset>

    <div class="actions">
      <button onclick={reset}>恢复默认</button>
      <button class="primary" onclick={onclose}>完成</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(560px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto; outline: none;
    background: var(--paper); border-radius: 10px; padding: 18px 24px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 8px; font-size: 20px; color: var(--accent); }
  fieldset { border: 1px solid var(--line); border-radius: 8px; margin: 12px 0; padding: 6px 14px 10px; }
  legend { padding: 0 6px; font-weight: 600; }
  .row { display: flex; gap: 12px; align-items: center; margin: 8px 0; flex-wrap: wrap; }
  .row > span { width: 72px; flex: none; }
  .row input[type="range"] { flex: 1; min-width: 140px; }
  .row input[type="text"] { flex: 1; min-width: 160px; }
  .row select { max-width: 340px; }
  output { width: 56px; text-align: right; color: var(--muted); }
  .seg { display: flex; }
  .seg button { border-radius: 0; margin-left: -1px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 6px 6px 0; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .sample {
    margin: 8px 0 0; padding: 10px 14px; background: var(--bg); border-radius: 6px;
    font-family: var(--reader-font, inherit); font-size: var(--reader-size, 19px); line-height: var(--reader-line, 2);
  }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
