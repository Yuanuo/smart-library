<script>
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";

  let { library, mode, building = false, onapply, onclose } = $props();
  let exts = $state("");
  let maxMb = $state(0);
  let report = $state(null);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state("");
  const title = $derived(`${mode === "rules" ? "收录规则" : "索引报告"}：${library.title}`);

  async function loadReport() {
    loading = true;
    error = "";
    try {
      report = await api.indexReport(library.id);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    exts = library.exts.join(", ");
    maxMb = library.max_mb;
    if (mode !== "report") return;
    loadReport();
    const off = api.onIndexDone((event) => {
      if (event.lib === library.id) loadReport();
    });
    return () => off.then((unlisten) => unlisten());
  });

  async function save(event) {
    event.preventDefault();
    if (!exts.trim() || maxMb == null || maxMb < 0 || saving || building) return;
    saving = true;
    error = "";
    try {
      await onapply(library.id, exts, Math.floor(maxMb));
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && !saving && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={title} tabindex="-1"
    onkeydown={(event) => event.key === "Escape" && !saving && onclose()}>
    <DialogHeader {title} path={library.source || library.title} />
    <form class="dialog-content" id="library-index-form" onsubmit={save}>
      {#if mode === "rules"}
        <label>文件后缀<input type="text" bind:value={exts} required disabled={saving} /></label>
        <label>单个文件最大大小（MB，0 为不限）<input type="number" min="0" step="1" bind:value={maxMb} required disabled={saving} /></label>
        <p class="muted">规则应用于目录和全文索引。Markdown、脑图和白板保留在目录中。保存后将重新建立全文索引。</p>
      {:else if loading}
        <p role="status">正在读取索引报告…</p>
      {:else if !error}
        {#if building}<p role="status">索引正在后台建立，完成后会刷新报告。</p>{/if}
        {#if report == null}
          <p class="muted">暂无索引报告。完成一次建立或重建索引后即可查看。</p>
        {:else if report.length}
          <p>{report.length} 项需要留意</p>
          <ul>
            {#each report as item}
              <li><strong>{item.metadata_only ? "仅收录文件信息" : "已跳过"}</strong>
                {#if item.path}<span>{item.path}</span>{/if}
                <span class="muted">{item.reason}</span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="muted">报告未记录跳过的文件或无法提取的正文。</p>
        {/if}
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </form>
    <div class="actions">
      <button type="button" onclick={onclose} disabled={saving}>{mode === "rules" ? "取消" : "关闭"}</button>
      {#if mode === "rules"}
        <button class="primary" type="submit" form="library-index-form" disabled={saving || building || !exts.trim()}>{saving ? "正在保存…" : "保存并重建索引"}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: #0005; display: grid; place-items: center; }
  .dialog { width: min(590px, calc(100vw - 32px)); max-height: calc(100vh - 48px); }
  label { display: grid; gap: 4px; margin-bottom: 12px; }
  input { width: 100%; }
  p { margin: 0 0 10px; }
  .error { color: var(--hl); }
  ul { margin: 0; padding: 0; list-style: none; }
  li { display: grid; gap: 3px; padding: 7px 0; border-top: 1px solid var(--line); overflow-wrap: anywhere; }
</style>
