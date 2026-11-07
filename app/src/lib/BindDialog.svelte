<script>
  // Bind knowledge libraries from a data source (design-spec 9a).
  // A CBETA bookcase: its official trees show the same data, so only one of
  // them can be bound (default 部類); its user-made trees are libraries of
  // their own and can be bound together.
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";

  let { initialPath = "", onclose, onbound } = $props();

  // step 1: kind of library; step 2: its parameters. Opening with a path
  // (bind more from the same bookcase) skips step 1.
  let kind = $state(initialPath ? "cbeta" : "");
  const KINDS = [
    { id: "cbeta", name: "CBETA 数据包", desc: "bookcase.zip 或解压后的目录，由其中的菜单关联数据（只读）", ready: true },
    { id: "zim", name: "Wikipedia 离线包（ZIM）", desc: "只做标题与全文索引，不展开目录（只读）。尚未开发。", ready: false },
    { id: "folder", name: "外部文件夹", desc: "链接本机文件夹，可按后缀、大小筛选并建全文索引（只读）", ready: true },
    { id: "notes", name: "自建知识库", desc: "内容由你手动添加和编辑，默认 Markdown（读写）", ready: true },
  ];
  let notesTitle = $state("");
  // external folder
  let folder = $state(null); // { path, name, conflict }
  let folderTitle = $state("");
  let exts = $state("txt, md, html, htm");
  let maxMb = $state(50);
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

  async function pickLibFolder() {
    const f = await open({ multiple: false, directory: true });
    if (!f) return;
    busy = true;
    error = "";
    try {
      folder = await api.inspectFolder(f);
      folderTitle = folder.name;
    } catch (e) {
      folder = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function bind() {
    busy = true;
    error = "";
    try {
      if (kind === "notes") {
        onbound(await api.createNotesLibrary(notesTitle));
        return;
      }
      if (kind === "folder") {
        const st = await api.addFolder(folder.path, folderTitle, exts, Math.max(0, Math.floor(Number(maxMb) || 0)));
        onbound(st);
        return;
      }
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
    <h2 id="bind-title">添加知识库</h2>
    {#if !kind}
      <p class="muted">选择知识库的类型：</p>
      {#each KINDS as k}
        <button class="kind" disabled={!k.ready} onclick={() => (kind = k.id)}>
          <strong>{k.name}</strong>
          <span class="muted">{k.desc}</span>
        </button>
      {/each}
    {:else if kind === "notes"}
      <p class="muted">新建一个空的知识库，之后在目录树里右键添加笔记。内容保存在本程序的数据目录里（一个 SQLite 文件）；它是你自己写的内容，没有原始数据可以重新导入，请注意备份。</p>
      <label class="field">名称 <input type="text" bind:value={notesTitle} placeholder="例如：读书笔记" /></label>
      {#if error}<p class="error">{error}</p>{/if}
    {:else if kind === "folder"}
      <p class="muted">链接一个本机文件夹。Smart Library 只读取其中的文件，不会修改它；子目录会按真实结构显示在目录树里。</p>
      <div class="row">
        <input type="text" readonly value={folder ? folder.path : ""} placeholder="尚未选择文件夹" aria-label="文件夹" />
        <button type="button" onclick={pickLibFolder}>选择文件夹…</button>
      </div>
      {#if folder?.conflict}<p class="error">{folder.conflict}</p>{/if}
      {#if error}<p class="error">{error}</p>{/if}
      {#if folder && !folder.conflict}
        <label class="field">名称 <input type="text" bind:value={folderTitle} /></label>
        <fieldset>
          <legend>索引哪些文件</legend>
          <label class="field">文件后缀（逗号或空格分隔）<input type="text" bind:value={exts} /></label>
          <p class="hint muted">txt、md、html 等文本读取全文；其他后缀（mp3、mp4、pdf 等）暂时只用文件名、类型、大小、日期作为内容。</p>
          <label class="field">单个文件最大 <input type="number" min="0" class="num" bind:value={maxMb} /> MB（0 为不限制）</label>
        </fieldset>
      {/if}
    {:else}
    <p class="muted">选择 CBETA 的 bookcase.zip（或已解压的目录）。Smart Library 只读取数据，不会修改它。</p>
    <form class="row" onsubmit={(e) => { e.preventDefault(); inspect(); }}>
      <input type="text" bind:value={path} placeholder="C:\…\bookcase.zip" aria-label="数据路径" />
      <button type="button" onclick={pickFile}>选择文件…</button>
      <button type="button" onclick={pickFolder}>选择文件夹…</button>
      <button class="primary" disabled={busy || !path.trim()}>读取</button>
    </form>

    {#if error && kind === "cbeta"}<p class="error">{error}</p>{/if}

    {#if src && kind === "cbeta"}
      <p class="muted small">CBETA {src.data_version}（{src.version}）· {src.path}</p>

      {#if cbetaTrees.length}
        <fieldset>
          <legend>CBETA 藏经（只能选一棵目录树）</legend>
          <p class="hint muted">这几棵目录树显示的是同一份数据，只绑定一棵。绑定后不能更换；要换，请先解除绑定再重新绑定。</p>
          {#each cbetaTrees as t}
            <label class="opt">
              <input type="radio" name="cbeta" value={t.id} bind:group={cbetaTree} disabled={cbetaBound} />
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

    {/if}

    <div class="actions">
      {#if kind && !initialPath}<button onclick={() => { kind = ""; src = null; error = ""; }}>上一步</button>{/if}
      <button onclick={onclose}>取消</button>
      {#if kind === "notes"}
        <button class="primary" disabled={!notesTitle.trim() || busy} onclick={bind}>创建</button>
      {:else if kind === "folder"}
        <button class="primary" disabled={!folder || !!folder.conflict || busy || !exts.trim()} onclick={bind}>绑定</button>
      {:else if kind}
        <button class="primary" disabled={!src || busy} onclick={bind}>绑定</button>
      {/if}
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
  .field { display: flex; gap: 8px; align-items: center; margin: 8px 0; flex-wrap: wrap; }
  .field input[type="text"] { flex: 1; min-width: 200px; }
  .num { width: 80px; }
  .kind { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%; text-align: left; margin: 6px 0; padding: 10px 14px; }
  .kind:disabled { opacity: 0.55; }
  .error { color: var(--hl); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
</style>
