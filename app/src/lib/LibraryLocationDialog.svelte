<script>
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";
  import { autofocus } from "./autofocus.js";
  let { title, libraryId = null, libraryIds = null, directoryAction = "选择当前目录", rootAction = null, onpick, onclose } = $props();
  let libraries = $state([]);
  let library = $state(null);
  let levels = $state([]);
  let nodes = $state([]);
  let filter = $state("");
  let loading = $state(true);
  let error = $state("");
  let list = $state();
  let seq = 0;
  const level = $derived(levels.at(-1));
  const path = $derived(library ? [library.title, ...levels.map((node) => node.title)].join(" › ") : "");
  const directories = $derived(nodes.filter((node) => node.lazy || node.children.length));
  const visible = $derived(directories.filter((node) => node.title.toLowerCase().includes(filter.trim().toLowerCase())));

  onMount(() => {
    const request = ++seq;
    api.status().then(({ libraries: sources }) => {
      if (request !== seq) return;
      libraries = libraryIds == null ? sources : sources.filter((source) => libraryIds.includes(source.id));
      if (libraryId == null) {
        loading = false;
        return;
      }
      const source = libraries.find((item) => item.id === libraryId);
      if (source && !source.error) browse(source);
      else {
        error = source?.error || "知识库不可用。";
        loading = false;
      }
    }).catch((e) => {
      if (request === seq) {
        error = String(e);
        loading = false;
      }
    });
    return () => { seq += 1; };
  });

  async function browse(source, trail = []) {
    const request = ++seq;
    library = source;
    levels = trail;
    filter = "";
    nodes = [];
    error = "";
    if (list) list.scrollTop = 0;
    if (!source) { loading = false; return; }
    const parent = trail.at(-1);
    loading = true;
    try {
      const items = source.kind === "zim" ? []
        : source.kind === "folder" || source.kind === "notes" ? await api.folderChildren(source.id, parent?.rel ?? "")
        : parent ? parent.children : await api.tree(source.id);
      if (request === seq) nodes = items;
    } catch (e) {
      if (request === seq) error = String(e);
    } finally {
      if (request === seq) loading = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" onkeydown={(event) => event.key === "Escape" && onclose()}>
    <DialogHeader {title} {path} />
    <div class="dialog-content">
      <div class="navigation">
        {#if libraryId == null}
          <label>知识库 <select use:autofocus value={library?.id ?? ""} onchange={(event) => browse(libraries.find((item) => item.id === event.currentTarget.value) ?? null)}>
            <option value="">选择知识库…</option>
            {#each libraries as item (item.id)}
              <option value={item.id} disabled={!!item.error}>{item.title}{item.error ? "（资料不可用）" : ""}</option>
            {/each}
          </select></label>
        {/if}
        {#if library && library.kind !== "zim"}
          <div class="directory-nav">
            {#if level}<button type="button" onclick={() => browse(library, levels.slice(0, -1))}>返回上一级</button>{/if}
            <input type="search" placeholder="筛选当前级的目录" aria-label="筛选目录" bind:value={filter} use:autofocus />
          </div>
        {/if}
      </div>
      <div class="directory-list" bind:this={list}>
        {#if error}
          <p class="error" role="alert">{error}</p>
        {:else if loading}
          <p class="muted" role="status">正在载入…</p>
        {:else if library && !directories.length}
          <p class="muted">当前层级没有下级目录。</p>
        {:else if library}
          <ul>
            {#each visible as node}
              <li><button type="button" onclick={() => browse(library, [...levels, node])}>{node.title} <span aria-hidden="true">›</span></button></li>
            {:else}
              <li class="muted">没有匹配的目录。</li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button type="button" class="primary" disabled={!library || (!level && !rootAction)} onclick={() => onpick({ library, node: level ?? null, path })}>
        {level ? directoryAction : rootAction ?? directoryAction}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: #0005; display: grid; place-items: center; }
  .dialog { width: min(500px, calc(100vw - 24px)); max-height: calc(100vh - 36px); }
  .dialog > .dialog-content { display: flex; flex-direction: column; overflow: hidden; }
  .navigation { flex: none; display: flex; flex-direction: column; gap: 8px; }
  label { display: flex; gap: 8px; align-items: center; }
  label select { flex: 1; min-width: 0; }
  .directory-nav { display: flex; gap: 8px; }
  .directory-nav button { flex: none; }
  .directory-nav input { flex: 1; min-width: 0; }
  .directory-list { min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  ul { list-style: none; padding: 0; margin: 6px 0 0; }
  li button { display: flex; width: 100%; justify-content: space-between; text-align: left; border: 0; background: none; padding: 6px 8px; }
  li button:hover { background: var(--accent-soft); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
  .error { color: var(--hl); }
</style>
