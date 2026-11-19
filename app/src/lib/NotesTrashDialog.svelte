<script>
  // The trash of an internal library: notes deleted with their subtree are kept
  // as `deleted` snapshots, and can be put back where they were.
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";
  import Icon from "./Icon.svelte";

  let { lib, libTitle = "", onclose, onrestored = null } = $props();
  let items = $state([]);
  let loading = $state(true);
  let busy = $state(""); // the id being restored, or "*" for all
  let error = $state("");
  let notice = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      items = await api.listDeletedNotes(lib);
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
    busy = it.id;
    error = "";
    notice = "";
    try {
      const r = await api.restoreDeletedNote(lib, it.id);
      await load(); // parents restored with it leave the list too
      let msg = `已恢复「${r.title}」${r.count > 1 ? `（${r.count} 篇笔记）` : ""}`;
      if (r.created?.length) msg += `；按原来的位置新建了上级 ${r.created.join("、")}`;
      if (r.relocated) msg += "；原来的上级已不在了，恢复到了最近的上级或顶层";
      notice = msg;
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
    const results = [];
    // restore one, then read the list again: restoring a note can bring a
    // deleted parent back too, whose row must not be restored a second time
    while (items.length) {
      const it = items[0];
      try {
        results.push(await api.restoreDeletedNote(lib, it.id));
        done += 1;
      } catch (e) {
        error = String(e);
        break;
      }
      const before = items.length;
      await load();
      if (error || items.length >= before) break; // the list did not move on
    }
    busy = "";
    if (done) {
      notice = `已恢复 ${done} 项`;
      // quiet: fold each restored node and load its way in, but do not jump
      for (const r of results) await onrestored?.(r, true);
      await onrestored?.(null); // refresh the tree afterwards
    }
  }

  async function remove(it) {
    if (busy) return;
    if (!(await ask(`永久删除「${it.title}」？\n它和下面的子笔记就再也找不回来了。`, {
      title: "永久删除", kind: "warning", okLabel: "永久删除", cancelLabel: "取消",
    }))) return;
    busy = it.id;
    error = "";
    notice = "";
    try {
      await api.deleteSnapshot(it.id);
      items = items.filter((x) => x.id !== it.id);
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }

  async function clearAll() {
    if (busy || !items.length) return;
    if (!(await ask(`清空回收站？\n${items.length} 项删除的笔记会永久删除，不能再恢复。`, {
      title: "清空回收站", kind: "warning", okLabel: "清空", cancelLabel: "取消",
    }))) return;
    busy = "*";
    error = "";
    notice = "";
    try {
      await api.purgeDeletedNotes(lib);
      items = [];
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="回收站" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <DialogHeader title={`回收站：${libTitle}`} />
    <div class="dialog-content">
      <p class="muted hint">删除的笔记和它下面的子笔记会放在这里，内容、属性、标签和别名都会一起恢复。恢复时按删除时的标题路径重建：上级还在就重用，不在就新建一个同名的空笔记；只有恢复的这一篇碰到同名标题时才加编号。回收站里的其他条目不受影响。笔记引用的图片会一直留到回收站被清空。</p>
      <div class="bar">
        <span class="muted">{loading ? "载入中…" : `${items.length} 项`}</span>
        <span class="grow"></span>
        <button type="button" onclick={load} disabled={!!busy || loading}>刷新</button>
        <button type="button" onclick={restoreAll} disabled={!!busy || loading || !items.length}>全部恢复</button>
        <button type="button" onclick={clearAll} disabled={!!busy || loading || !items.length}>清空回收站</button>
      </div>
      {#if error}<p class="error">{error}</p>{/if}
      {#if notice}<p class="notice">{notice}</p>{/if}
      {#if !loading && !items.length}
        <p class="muted empty">回收站是空的。</p>
      {:else if items.length}
        <ul class="items">
          {#each items as it (it.id)}
            <li>
              <span class="kind"><Icon name="file" size={16} /></span>
              <div class="meta">
                <div class="path" title={it.title}>{it.title}</div>
                <div class="sub muted">
                  {it.count > 1 ? `共 ${it.count} 篇笔记` : "笔记"} · 删除于 {ago(it.created_at)}
                </div>
              </div>
              <button type="button" onclick={() => restore(it)} disabled={!!busy}>
                {busy === it.id ? "恢复中…" : "恢复"}
              </button>
              <button type="button" class="danger" onclick={() => remove(it)} disabled={!!busy} title="永久删除">删除</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <div class="actions"><button type="button" class="primary" onclick={onclose}>关闭</button></div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3); display: flex; align-items: center; justify-content: center; }
  .dialog { width: min(600px, calc(100vw - 32px)); }
  .hint { margin: 0 0 10px; font-size: 13px; line-height: 1.6; }
  .bar { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; flex-wrap: wrap; }
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
  .danger { color: var(--hl); }
</style>
