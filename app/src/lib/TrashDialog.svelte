<script>
  // The trash of a read-write local folder: deleted files and folders, with the
  // place they came from, ready to be put back. The pictures a restored note
  // uses come back with it.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";
  import Icon from "./Icon.svelte";

  let { lib, libTitle = "", onclose, onrestored = null } = $props();
  let items = $state([]);
  let loading = $state(true);
  let busy = $state(""); // the entry being restored, or "*" for all
  let error = $state("");
  let notice = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      items = await api.trashList(lib);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  function ago(t) {
    if (!t) return "";
    const s = Math.max(0, Date.now() / 1000 - t);
    if (s < 60) return "刚刚";
    if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
    if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
    if (s < 86400 * 30) return `${Math.floor(s / 86400)} 天前`;
    return new Date(t * 1000).toLocaleDateString("zh-CN");
  }

  async function restore(it) {
    if (busy) return;
    busy = it.entry;
    error = "";
    notice = "";
    try {
      const r = await api.trashRestore(lib, it.entry);
      items = items.filter((x) => x.entry !== it.entry);
      notice = `已恢复「${r.rel}」${r.pictures ? `，带回 ${r.pictures} 张图片` : ""}`;
      onrestored?.(r);
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }

  async function restoreAll() {
    if (busy || !items.length) return;
    busy = "*";
    error = "";
    notice = "";
    let done = 0;
    for (const it of [...items]) {
      try {
        await api.trashRestore(lib, it.entry);
        items = items.filter((x) => x.entry !== it.entry);
        done += 1;
      } catch (e) {
        error = String(e);
      }
    }
    busy = "";
    if (done) {
      notice = `已恢复 ${done} 项`;
      onrestored?.();
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="回收站" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <DialogHeader title={`回收站：${libTitle}`} />
    <div class="dialog-content">
      <p class="muted hint">删除的文件和文件夹可以恢复到原来的位置。「设置 → 数据与备份」可以永久清空文件夹库的回收站。</p>
      <div class="bar">
        <span class="muted">{loading ? "载入中…" : `${items.length} 项`}</span>
        <span class="grow"></span>
        <button type="button" onclick={load} disabled={!!busy || loading}>刷新</button>
        <button type="button" onclick={restoreAll} disabled={!!busy || loading || !items.length}>全部恢复</button>
      </div>
      {#if error}<p class="error">{error}</p>{/if}
      {#if notice}<p class="notice">{notice}</p>{/if}
      {#if !loading && !items.length}
        <p class="muted empty">回收站是空的。</p>
      {:else if items.length}
        <ul class="items">
          {#each items as it (it.entry)}
            <li>
              <span class="kind"><Icon name={it.kind === "dir" ? "folder" : "file"} size={16} /></span>
              <div class="meta">
                <div class="path" title={it.rel}>{it.rel}</div>
                <div class="sub muted">
                  {it.kind === "dir" ? "文件夹" : "文件"} · {it.time ? `删除于 ${ago(it.time)}` : "较早版本删除"}
                </div>
              </div>
              <button type="button" onclick={() => restore(it)} disabled={!!busy}>
                {busy === it.entry ? "恢复中…" : "恢复"}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if items.some((it) => !it.time)}
        <p class="muted hint">较早版本删除的条目没有记录原位置，会恢复到知识库顶层。</p>
      {/if}
      <p class="muted hint">文件夹会合并回同名的文件夹；只有同名的文件会加上编号（如「笔记.md」变成「笔记 2.md」），不会被覆盖。</p>
    </div>
    <div class="actions"><button type="button" class="primary" onclick={onclose}>关闭</button></div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3); display: flex; align-items: center; justify-content: center; }
  .dialog { width: min(600px, calc(100vw - 32px)); }
  .hint { margin: 0 0 10px; font-size: 13px; line-height: 1.6; }
  .bar { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; }
  .grow { flex: 1; }
  .error { color: var(--hl); font-size: 13px; margin: 4px 0; }
  .notice { color: var(--accent); font-size: 13px; margin: 4px 0; }
  .empty { padding: 26px 0; text-align: center; }
  .items { list-style: none; margin: 0; padding: 0; border: 1px solid var(--line); border-radius: 6px; }
  .items li { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-bottom: 1px solid var(--line); }
  .items li:last-child { border-bottom: none; }
  .kind { flex: none; display: inline-flex; color: var(--muted); }
  .meta { flex: 1; min-width: 0; }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 12px; margin-top: 1px; }
  .items button { flex: none; }
</style>
