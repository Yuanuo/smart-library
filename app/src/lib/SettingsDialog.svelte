<script>
  // Settings: theme, interface zoom and font, reader font, size and line
  // height. Changes apply at once and are kept.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { settings, update, reset, FONTS } from "./settings.svelte.js";

  let { onclose } = $props();

  // clearing what the program leaves behind (trash of local folders, pictures nothing refers to)
  let cleaning = $state(false);
  let cleanMsg = $state("");
  async function clean() {
    const note = "会清空读写模式本地文件夹里的 .smart-library/trash（删除的文件和文件夹），并删除没有任何笔记引用的图片。";
    if (!(await ask(`清理缓存？\n${note}\n清空后不能再从回收站找回。`, { title: "清理缓存", kind: "warning", okLabel: "清理", cancelLabel: "取消" }))) return;
    cleaning = true;
    cleanMsg = "";
    try {
      cleanMsg = await api.clearCache();
    } catch (e) {
      cleanMsg = String(e);
    } finally {
      cleaning = false;
    }
  }

  // the table of periods (时域): the user's rows on top of the preset ones
  let dyn = $state(null); // null: closed; otherwise { text, preset, msg }
  async function openDyn() {
    try {
      const [user, preset] = await api.dynastyText();
      dyn = { text: user || preset, preset, msg: user ? "" : "显示的是预设表；改动后保存即成为你的表（同名的行覆盖预设）。" };
    } catch (e) {
      dyn = { text: "", preset: "", msg: String(e) };
    }
  }
  async function saveDyn() {
    try {
      const n = await api.setDynastyText(dyn.text);
      dyn.msg = `已保存，${n} 行。已建好的索引要重建后，别称才会合并。`;
    } catch (e) {
      dyn.msg = String(e);
    }
  }
  async function exportDyn() {
    const path = await saveDialog({ defaultPath: "时域表.txt", filters: [{ name: "文本", extensions: ["txt"] }] });
    if (!path) return;
    try {
      await api.saveBytes(path, new TextEncoder().encode(dyn.text));
      dyn.msg = `已导出：${path}`;
    } catch (e) {
      dyn.msg = String(e);
    }
  }
  async function importDyn() {
    const path = await openDialog({ multiple: false, filters: [{ name: "文本", extensions: ["txt", "tsv"] }] });
    if (!path) return;
    try {
      dyn.text = await api.readTextFile(path);
      dyn.msg = "已读入，核对后点「保存」。";
    } catch (e) {
      dyn.msg = String(e);
    }
  }

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
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="设置" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>设置</h2></header>
    <div class="dialog-content">

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
      <div class="row">
        <span>ZIM 外部链接</span>
        <select value={settings.zimExternal} onchange={(e) => update({ zimExternal: e.currentTarget.value })}>
          <option value="ask">每次询问</option><option value="always">直接在浏览器打开</option>
        </select>
      </div>
    </fieldset>

    <fieldset>
      <legend>搜索</legend>
      <div class="row">
        <span>快搜每批</span>
        <input type="range" min="50" max="1000" step="50" value={settings.quickLimit}
          oninput={(e) => update({ quickLimit: Number(e.currentTarget.value) })} />
        <output>{settings.quickLimit} 项</output>
      </div>
      <p class="muted hint">标题快搜先显示这么多条，列表滚到底（或用键盘走到末尾）时自动再加载一批。</p>
    </fieldset>

    <fieldset>
      <legend>时域表</legend>
      <p class="muted hint">朝代的名称、起止年份和别称（别称在索引时并入第一列的名称，搜索页的「时域」按起年排序）。可导出、导入。</p>
      {#if !dyn}
        <div class="row"><button onclick={openDyn}>编辑时域表…</button></div>
      {:else}
        <textarea class="dyn" bind:value={dyn.text} spellcheck="false" aria-label="时域表"></textarea>
        <div class="row">
          <button onclick={saveDyn}>保存</button>
          <button onclick={() => (dyn.text = dyn.preset)}>恢复预设</button>
          <button onclick={importDyn}>导入…</button>
          <button onclick={exportDyn}>导出…</button>
          <button onclick={() => (dyn = null)}>收起</button>
        </div>
        {#if dyn.msg}<p class="muted hint">{dyn.msg}</p>{/if}
      {/if}
    </fieldset>

    <fieldset>
      <legend>每日一句</legend>
      <label class="row">
        <input type="checkbox" checked={settings.dailyOn} onchange={(e) => update({ dailyOn: e.currentTarget.checked })} />
        <span class="wide">每天从知识库里随机抽一句，在目录树下方的喇叭按钮上提示</span>
      </label>
      <div class="row">
        <span>来源</span>
        <div class="sources">
          {#each settings.dailySources as src, i}
            <div class="src" title={src.label}>
              <span class="lab">{src.label}</span>
              <button class="link" onclick={() => update({ dailySources: settings.dailySources.filter((_, j) => j !== i) })} aria-label="移除">×</button>
            </div>
          {:else}
            <span class="muted">全部知识库</span>
          {/each}
        </div>
      </div>
      <p class="muted hint">在目录树里右键知识库或节点，选「加入每日一句来源」；没有来源时从全部已建索引的知识库里抽。</p>
    </fieldset>

    <fieldset>
      <legend>清理</legend>
      <div class="row">
        <button onclick={clean} disabled={cleaning}>{cleaning ? "清理中…" : "清理缓存"}</button>
      </div>
      <p class="muted hint">清空本地文件夹（读写模式）里的回收站 .smart-library/trash，并删除没有笔记引用的图片（内部知识库也一样）。</p>
      {#if cleanMsg}<p class="hint" style="white-space: pre-line">{cleanMsg}</p>{/if}
    </fieldset>

    </div>
    <div class="actions">
      <button onclick={reset}>恢复默认</button>
      <button class="primary" onclick={onclose}>完成</button>
    </div>
  </div>
</div>

<style>
  .dyn { width: 100%; height: 160px; font-family: monospace; font-size: 12px; box-sizing: border-box; }
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(560px, calc(100vw - 32px)); }
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
  .wide { width: auto !important; flex: 1; }
  .sources { flex: 1; min-width: 0; }
  .src { display: flex; align-items: center; gap: 6px; padding: 2px 0; border-bottom: 1px dashed var(--line); }
  .src .lab { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { margin: 0 0 6px; font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
