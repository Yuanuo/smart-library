<script>
  // Bind knowledge libraries from a data source (design-spec 9a).
  // A CBETA bookcase: its official trees show the same data, so only one of
  // them can be bound (default 部類); its user-made trees are libraries of
  // their own and can be bound together.
  import { onMount, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import { confirmWritable } from "./writable.js";

  let { initialPath = "", onclose, onbound } = $props();

  // step 1: kind of library; step 2: its parameters. Opening with a path
  // (bind more from the same bookcase) skips step 1.
  let kind = $state(untrack(() => (initialPath ? "cbeta" : "")));
  const KINDS = [
    { id: "notes", name: "内部知识库", desc: "笔记存在程序自己的数据库里，不依赖文件夹：适合大量笔记和日记，保存与搜索都快；Markdown、脑图、白板，可整体导出备份（读写）", ready: true },
    { id: "folder", name: "本地文件夹", desc: "直接使用磁盘上的文件夹和文件：可用 git 或网盘同步备份，别的软件也能打开；默认只读，可解锁后编辑 Markdown、脑图、白板（可控读写）", ready: true },
    { id: "cbeta", name: "CBETA 数据包", desc: "bookcase.zip 或解压后的目录，由其中的菜单关联数据（只读）", ready: true },
    { id: "zim", name: "Wikipedia 离线包（ZIM）", desc: "只做标题与全文索引，不展开目录（只读）。", ready: true },
  ];
  let notesTitle = $state("");
  // external folder
  let folder = $state(null); // { path, name, conflict }
  let folderTitle = $state("");
  let exts = $state("txt, md, html, htm, pdf, docx");
  let maxMb = $state(50);
  let writable = $state(false); // read-write mode: the user must accept the rules (confirmWritable)
  let path = $state("");
  let libraryId = $state("");
  let libraryIds = $state({});
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
        onbound(await api.createNotesLibrary(notesTitle, libraryId));
        return;
      }
      if (kind === "zim") {
        onbound(await api.addZim(path, libraryId));
        return;
      }
      if (kind === "folder") {
        if (writable && !(await confirmWritable(folderTitle || folder.name))) return;
        const st = await api.addFolder(folder.path, folderTitle, exts, Math.max(0, Math.floor(Number(maxMb) || 0)), writable, libraryId);
        onbound(st);
        return;
      }
      const newHtml = htmlTrees.filter((t) => html[t.id] && !t.bound_as).map((t) => t.id);
      const st = await api.bindLibraries(src.path, cbetaTree || null, newHtml, libraryIds);
      onbound(st);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-labelledby="bind-title">
    <header class="dialog-header"><h2 id="bind-title">添加知识库</h2></header>
    <div class="dialog-content">
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
      <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder="留空自动生成" /></label>
      {#if error}<p class="error">{error}</p>{/if}
    {:else if kind === "folder"}
      <p class="muted">链接一个本机文件夹，子目录会按真实结构显示在目录树里。默认只读，Smart Library 不会修改其中的文件；也可以选择读写模式，直接编辑里面的笔记。</p>
      <div class="row">
        <input type="text" readonly value={folder ? folder.path : ""} placeholder="尚未选择文件夹" aria-label="文件夹" />
        <button type="button" onclick={pickLibFolder}>选择文件夹…</button>
      </div>
      {#if folder?.conflict}<p class="error">{folder.conflict}</p>{/if}
      {#if error}<p class="error">{error}</p>{/if}
      {#if folder && !folder.conflict}
        <label class="field">名称 <input type="text" bind:value={folderTitle} /></label>
        <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder="留空自动生成" /></label>
        <fieldset>
          <legend>索引哪些文件</legend>
          <label class="field">文件后缀（逗号或空格分隔）<input type="text" bind:value={exts} /></label>
          <p class="hint muted">txt、md、html 等文本，以及 pdf、docx、pptx、odt 读取全文（旧的 .doc 不支持；扫描的 PDF 没有文字）；其他后缀（mp3、mp4 等）只用文件名、类型、大小、日期作为内容。</p>
          <label class="field">单个文件最大 <input type="number" min="0" class="num" bind:value={maxMb} /> MB（0 为不限制）</label>
        </fieldset>
        <fieldset>
          <legend>读写模式</legend>
          <label class="opt">
            <input type="checkbox" bind:checked={writable} />
            允许在软件里编辑这个文件夹里的笔记（.md、脑图 .map.json、白板 .wbd.json）
          </label>
          <p class="hint muted">
            {writable ? "点「绑定」时会先说明规则和风险，需要你确认。之后也可以在知识库的右键菜单里随时切换。" : "不勾选则保持只读：不会新建、修改或删除任何文件。"}
          </p>
        </fieldset>
      {/if}
    {:else if kind === "zim"}
      <p class="muted">选择 ZIM 文件。数据包只读，绑定后可搜索标题并建立全文索引。</p>
      <div class="row">
        <input type="text" bind:value={path} placeholder="C:\\…\\wikipedia.zim" aria-label="ZIM 文件" />
        <button type="button" onclick={async () => { const f = await open({ multiple: false, filters: [{ name: "ZIM", extensions: ["zim"] }] }); if (f) { path = f; const name = f.split(/[\\/]/).pop().toLowerCase(); libraryId = ["wikipedia", "wikiquote", "wikibooks", "wikisource", "wiktionary", "wikivoyage", "wikinews", "wikiversity"].find((id) => name.startsWith(id + "_")) ?? ""; } }}>选择文件…</button>
      </div>
      <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder="例如 wikipedia、wikiquote" /></label>
      <p class="hint muted">跨维基站点链接按知识库 ID 查找，例如 wikipedia。</p>
      {#if error}<p class="error">{error}</p>{/if}
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
        {#if cbetaTree && !cbetaBound}<label class="field">CBETA 知识库 ID（可选）<input type="text" bind:value={libraryIds[cbetaTree]} placeholder="留空自动生成" /></label>{/if}
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
            {#if html[t.id] && !t.bound_as}<label class="field">{t.title} 的知识库 ID（可选）<input type="text" bind:value={libraryIds[t.id]} placeholder="留空自动生成" /></label>{/if}
          {/each}
        </fieldset>
      {/if}

      {#if otherTrees.length}
        <p class="muted small">没有可读内容的目录树：{otherTrees.map((t) => t.title).join("、")}</p>
      {/if}
    {/if}

    {/if}

    {#if kind}<p class="hint muted">知识库 ID 只能使用小写字母、数字、连字符或下划线，创建后不可更改。</p>{/if}

    </div>
    <div class="actions">
      {#if kind && !initialPath}<button onclick={() => { kind = ""; src = null; error = ""; }}>上一步</button>{/if}
      <button onclick={onclose}>取消</button>
      {#if kind === "notes"}
        <button class="primary" disabled={!notesTitle.trim() || busy} onclick={bind}>创建</button>
      {:else if kind === "folder"}
        <button class="primary" disabled={!folder || !!folder.conflict || busy || !exts.trim()} onclick={bind}>绑定</button>
      {:else if kind === "zim"}
        <button class="primary" disabled={!path.trim() || busy} onclick={bind}>绑定</button>
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
  .dialog { width: min(720px, calc(100vw - 32px)); }
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
