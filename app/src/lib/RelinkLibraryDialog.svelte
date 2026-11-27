<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { untrack } from "svelte";
  import DialogHeader from "./DialogHeader.svelte";
  import { autofocus } from "./autofocus.js";

  let { library, onapply, onclose } = $props();
  let path = $state(untrack(() => library.source || ""));
  let busy = $state(false);
  let error = $state("");
  const title = $derived(`重新定位资料源：${library.title}`);

  async function pick(directory) {
    try {
      const filters = directory ? undefined : [{ name: library.kind === "zim" ? "ZIM" : "CBETA 数据包", extensions: library.kind === "zim" ? ["zim"] : ["zip"] }];
      const result = await open({ directory, filters });
      if (typeof result === "string") path = result;
    } catch (e) {
      error = String(e);
    }
  }

  async function apply(event) {
    event.preventDefault();
    if (!path.trim() || busy) return;
    busy = true;
    error = "";
    try {
      await onapply(library, path);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && !busy && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" onkeydown={(event) => event.key === "Escape" && !busy && onclose()}>
    <DialogHeader {title} path={library.source} />
    <form id="relink-form" class="dialog-content" onsubmit={apply}>
      <p>{library.remote ? "选择同一个 Git 仓库在本机的新位置。" : "选择同一份资料的新位置。"} 知识库 ID 和关联记录会保留，全文索引会重新建立。</p>
      <label>新路径<input type="text" use:autofocus={{ select: true }} bind:value={path} required disabled={busy} /></label>
      <div class="browse">
        {#if library.kind !== "zim"}<button type="button" onclick={() => pick(true)} disabled={busy}>选择文件夹…</button>{/if}
        {#if library.kind !== "folder"}<button type="button" onclick={() => pick(false)} disabled={busy}>选择数据包…</button>{/if}
      </div>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </form>
    <div class="actions">
      <button type="button" onclick={onclose} disabled={busy}>取消</button>
      <button type="submit" form="relink-form" class="primary" disabled={busy || !path.trim()}>{busy ? "正在验证资料源…" : "使用新位置"}</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: #0005; display: grid; place-items: center; }
  .dialog { width: min(590px, calc(100vw - 24px)); }
  p { margin: 0 0 10px; }
  label { display: grid; gap: 4px; }
  .browse { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .error { margin-top: 10px; color: var(--hl); }
</style>
