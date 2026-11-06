<script>
  // Bind knowledge libraries from a data source (design-spec 9a).
  // A CBETA bookcase: its official trees show the same data, so only one of
  // them can be bound (default 部類); its user-made trees are libraries of
  // their own and can be bound together.
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";

  let { initialPath = "", onclose, onbound } = $props();

  let path = $state("");
  let src = $state(null);
  let error = $state("");
  let busy = $state(false);
  let cbetaTree = $state(null); // tree id, or "" for none
  let html = $state({}); // tree id -> checked

  const cbetaTrees = $derived(src ? src.trees.filter((t) => t.group === "cbeta") : []);
  const htmlTrees = $derived(src ? src.trees.filter((t) => t.group === "html") : []);
  const otherTrees = $derived(src ? src.trees.filter((t) => t.group === "other") : []);
  const cbetaBound = $derived(cbetaTrees.some((t) => t.bound_as));

  onMount(() => {
    path = initialPath;
    if (path) inspect();
  });

  async function pickFile() {
    const f = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "CBETA bookcase", extensions: ["zip"] }],
    });
    if (f) {
      path = f;
      inspect();
    }
  }

  async function pickFolder() {
    const f = await open({ multiple: false, directory: true });
    if (f) {
      path = f;
      inspect();
    }
  }

  async function inspect() {
    if (!path.trim()) return;
    busy = true;
    error = "";
    try {
      src = await api.inspectSource(path);
      cbetaTree = src.default_cbeta_tree ?? "";
      html = Object.fromEntries(src.trees.filter((t) => t.group === "html").map((t) => [t.id, !!t.bound_as]));
    } catch (e) {
      src = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function bind() {
    busy = true;
    error = "";
    try {
      const newHtml = htmlTrees.filter((t) => html[t.id] && !t.bound_as).map((t) => t.id);
      const st = await api.bindLibraries(src.path, cbetaTree || null, newHtml);
      onbound(st);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="bind-title">
    <h2 id="bind-title">绑定知识库</h2>
    <p class="muted">选择 CBETA 的 bookcase.zip（或已解压的目录）。Smart Library 只读取数据，不会修改它。</p>
    <form class="row" onsubmit={(e) => { e.preventDefault(); inspect(); }}>
      <input type="text" bind:value={path} placeholder="C:\…\bookcase.zip" aria-label="数据路径" />
      <button type="button" onclick={pickFile}>选择文件…</button>
      <button type="button" onclick={pickFolder}>选择文件夹…</button>
      <button class="primary" disabled={busy || !path.trim()}>读取</button>
    </form>

    {#if error}<p class="error">{error}</p>{/if}

    {#if src}
      <p class="muted small">CBETA {src.data_version}（{src.version}）· {src.path}</p>

      {#if cbetaTrees.length}
        <fieldset>
          <legend>CBETA 藏经（只能选一棵目录树）</legend>
          <p class="hint muted">这几棵目录树显示的是同一份数据，只绑定一棵；以后可以换成另一棵，不需要重建索引。</p>
          {#each cbetaTrees as t}
            <label class="opt">
              <input type="radio" name="cbeta" value={t.id} bind:group={cbetaTree} />
              {t.title} <span class="muted">· {t.leaves} 个条目{t.bound_as ? " · 当前绑定" : ""}</span>
            </label>
          {/each}
          {#if !cbetaBound}
            <label class="opt">
              <input type="radio" name="cbeta" value="" bind:group={cbetaTree} />
              <span class="muted">不绑定 CBETA</span>
            </label>
          {/if}
        </fieldset>
      {/if}

      {#if htmlTrees.length}
        <fieldset>
          <legend>其他知识库（可以同时绑定）</legend>
          <p class="hint muted">数据各不相同，每个知识库有自己的索引。</p>
          {#each htmlTrees as t}
            <label class="opt">
              <input type="checkbox" bind:checked={html[t.id]} disabled={!!t.bound_as} />
              {t.title} <span class="muted">· a/{t.code}/ · {t.leaves} 篇{t.bound_as ? " · 已绑定" : ""}</span>
            </label>
          {/each}
        </fieldset>
      {/if}

      {#if otherTrees.length}
        <p class="muted small">没有可读内容的目录树：{otherTrees.map((t) => t.title).join("、")}</p>
      {/if}
    {/if}

    <div class="actions">
      <button onclick={onclose}>取消</button>
      <button class="primary" disabled={!src || busy} onclick={bind}>绑定</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 900; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(720px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto;
    background: var(--paper); border-radius: 10px; padding: 20px 24px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 6px; font-size: 20px; color: var(--accent); }
  .row { display: flex; gap: 8px; margin: 12px 0; flex-wrap: wrap; }
  .row input { flex: 1; min-width: 240px; }
  fieldset { border: 1px solid var(--line); border-radius: 8px; margin: 12px 0; padding: 8px 14px 10px; }
  legend { padding: 0 6px; font-weight: 600; }
  .hint { margin: 2px 0 6px; font-size: 13px; }
  .opt { display: flex; gap: 8px; align-items: center; padding: 4px 0; }
  .small { font-size: 13px; }
  .error { color: var(--hl); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
</style>
