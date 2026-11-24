<script>
  // Bind knowledge libraries from a data source (design-spec 9a).
  // A CBETA bookcase: its official trees show the same data, so only one of
  // them can be bound (default 部類); its user-made trees are libraries of
  // their own and can be bound together.
  import { onMount, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import { confirmWritable } from "./writable.js";
  import { examplePath, IS_WIN } from "./platform.js";
  import { autofocus } from "./autofocus.js";

  let { initialPath = "", initialKind = "", existingTitles = [], onclose, onbound } = $props();

  function uniqueTitle(base, fallback = "我的知识库") {
    const b = (base || "").trim() || fallback;
    const taken = new Set(existingTitles);
    if (!taken.has(b)) return b;
    let n = 2;
    while (taken.has(`${b} ${n}`)) n += 1;
    return `${b} ${n}`;
  }

  function fileStem(p) {
    const name = (p || "").split(/[\\/]/).pop() || "";
    return name.replace(/\.(smartlib|smartdir|zip)$/i, "").trim();
  }

  function enterKind(k) {
    kind = k;
    error = "";
    if (k === "notes" && !notesTitle.trim()) notesTitle = uniqueTitle("我的知识库");
    if (k === "git") checkGit();
  }

  // step 1: kind of library; step 2: its parameters. Opening with a path
  // (bind more from the same bookcase) or a shortcut kind skips step 1.
  let kind = $state(untrack(() => (initialPath ? "cbeta" : initialKind)));
  const KIND_GROUPS = [
    {
      title: "新建与同步",
      items: [
        { id: "notes", name: "默认知识库（推荐）", desc: "笔记存在程序自己的数据库里，不依赖文件夹：适合大量笔记和日记，保存与搜索都快；Markdown、脑图、白板，可整体导出备份（读写）", ready: true },
        { id: "folder", name: "本地文件夹", desc: "直接使用磁盘上的文件夹和文件：可用 git 或网盘同步备份，别的软件也能打开；默认只读，可解锁后编辑 Markdown、脑图、白板（可控读写）", ready: true },
        { id: "git", name: "远程知识库（Git）", desc: "克隆远程 Git 仓库为本地只读知识库（默认浅克隆），支持手动与定期检查更新（只读）", ready: true },
      ],
    },
    {
      title: "数据包与导入",
      items: [
        { id: "cbeta", name: "CBETA 数据包", desc: "bookcase.zip 或解压后的目录，由其中的菜单关联数据（只读）", ready: true },
        { id: "zim", name: "Wikipedia 离线包（ZIM）", desc: "只做标题与全文索引，不展开目录（只读）。", ready: true },
        { id: "import", name: "导入知识库", desc: "从导出的知识库数据包（.smartlib / .smartdir）导入为新的知识库", ready: true },
      ],
    },
  ];
  let notesTitle = $state("");
  // remote git library
  let gitInfo = $state(null); // { available, path, version }
  let gitChecking = $state(false);
  let gitEditingPath = $state(false);
  let customGitPath = $state("");
  let gitUrl = $state("");
  let gitTitle = $state("");
  let gitBranch = $state("");
  let gitDest = $state("");
  let gitDestEdited = $state(false);
  let gitTitleEdited = $state(false);
  let gitUrlTimer = null;
  // import package (.smartlib / .smartdir)
  let pkgPath = $state("");
  let pkgInfo = $state(null); // { kind, title, scope, files, exts, max_mb }
  let importMode = $state("folder"); // "folder" | "notes" (for .smartdir)
  let importTitle = $state("");
  let importDest = $state("");
  let importWritable = $state(false);
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
  let zimInfo = $state(null); // header of the chosen ZIM, once it checks out
  let zimError = $state("");

  const cbetaTrees = $derived(src ? src.trees.filter((t) => t.group === "cbeta") : []);
  const htmlTrees = $derived(src ? src.trees.filter((t) => t.group === "html") : []);
  const otherTrees = $derived(src ? src.trees.filter((t) => t.group === "other") : []);
  const cbetaBound = $derived(cbetaTrees.some((t) => t.bound_as));
  const ID_PLACEHOLDER = "留空自动生成（仅小写字母、数字、-、_，创建后不可改）";
  const ID_TITLE = "知识库 ID 只能使用小写字母、数字、连字符或下划线，创建后不可更改";

  // what the window is waiting for right now: { label, done, total }
  let progress = $state(null);
  let inspectTimer = null;
  let zimTimer = null;
  onMount(() => {
    const unlisten = api.onBindProgress((p) => (progress = p));
    path = initialPath;
    if (path) inspect();
    if (kind === "notes" && !notesTitle.trim()) notesTitle = uniqueTitle("我的知识库");
    if (kind === "git") checkGit();
    return () => {
      unlisten.then((f) => f());
      clearTimeout(inspectTimer);
      clearTimeout(zimTimer);
      clearTimeout(gitUrlTimer);
    };
  });

  /// Read the source once the typed path settles. Picking a file or folder in
  /// the system dialog reads it at once; there is no 读取 button any more.
  function laterInspect() {
    src = null;
    error = "";
    clearTimeout(inspectTimer);
    inspectTimer = setTimeout(() => {
      if (path.trim()) inspect();
    }, 500);
  }

  /// ZIM: check the file (header only, quick) as soon as it is picked or the
  /// path settles; 「绑定」stays disabled until it checks out.
  function laterInspectZim() {
    zimInfo = null;
    zimError = "";
    if (!libraryId.trim()) libraryId = guessWikiId(path);
    clearTimeout(zimTimer);
    zimTimer = setTimeout(() => {
      if (path.trim()) inspectZimFile();
    }, 500);
  }

  async function inspectZimFile() {
    if (!path.trim()) return;
    busy = true;
    progress = { label: "正在检查 ZIM…", done: 0, total: 0 };
    zimInfo = null;
    zimError = "";
    try {
      zimInfo = await api.inspectZim(path);
    } catch (e) {
      zimError = String(e);
    } finally {
      busy = false;
      progress = null;
    }
  }

  async function pickZim() {
    const f = await open({ multiple: false, filters: [{ name: "ZIM", extensions: ["zim"] }] });
    if (!f) return;
    clearTimeout(zimTimer);
    path = f;
    libraryId = guessWikiId(f);
    inspectZimFile();
  }

  /// The Wikimedia project a ZIM belongs to, from its file name
  /// (`wikipedia_zh_all_2023-01.zim` → `wikipedia`); cross-wiki links use it.
  const WIKIS = ["wikipedia", "wikiquote", "wikibooks", "wikisource", "wiktionary", "wikivoyage", "wikinews", "wikiversity"];
  function guessWikiId(p) {
    const name = (p || "").split(/[\\/]/).pop().toLowerCase();
    return WIKIS.find((id) => name.startsWith(id + "_")) ?? "";
  }

  const sizeText = (n) => (n >= 1073741824 ? `${(n / 1073741824).toFixed(1)} GB` : `${Math.max(1, Math.round(n / 1048576))} MB`);

  async function pickFile() {
    const f = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "CBETA bookcase", extensions: ["zip"] }],
    });
    if (f) {
      clearTimeout(inspectTimer);
      path = f;
      inspect();
    }
  }

  async function pickFolder() {
    const f = await open({ multiple: false, directory: true });
    if (f) {
      clearTimeout(inspectTimer);
      path = f;
      inspect();
    }
  }

  async function inspect() {
    if (!path.trim()) return;
    busy = true;
    error = "";
    progress = { label: "正在读取数据包…", done: 0, total: 0 };
    try {
      src = await api.inspectSource(path);
      cbetaTree = src.default_cbeta_tree ?? "";
      html = Object.fromEntries(src.trees.filter((t) => t.group === "html").map((t) => [t.id, !!t.bound_as]));
    } catch (e) {
      src = null;
      error = String(e);
    } finally {
      busy = false;
      progress = null;
    }
  }

  async function pickLibFolder() {
    const f = await open({ multiple: false, directory: true });
    if (!f) return;
    busy = true;
    error = "";
    try {
      folder = await api.inspectFolder(f);
      folderTitle = uniqueTitle(folder.name, "本地文件夹");
    } catch (e) {
      folder = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function pickPackage() {
    if (busy) return;
    error = "";
    const f = await open({
      multiple: false,
      title: "选择数据包",
      filters: [
        { name: "知识库数据包（*.smartlib, *.smartdir）", extensions: ["smartlib", "smartdir"] },
        { name: "文件夹数据包（*.smartdir）", extensions: ["smartdir"] },
        { name: "ZIP", extensions: ["zip"] },
      ],
    });
    if (!f) return;
    busy = true;
    progress = { label: "正在读取数据包…", done: 0, total: 0 };
    pkgPath = f;
    pkgInfo = null;
    try {
      pkgInfo = await api.inspectPackage(f);
      importTitle = uniqueTitle(pkgInfo.title || fileStem(f), "导入的知识库");
      importMode = pkgInfo.kind === "smartdir" ? "folder" : "notes";
      importDest = pkgInfo.default_dest || "";
      importWritable = false;
    } catch (e) {
      pkgInfo = null;
      error = String(e);
    } finally {
      busy = false;
      progress = null;
    }
  }

  async function pickImportDest() {
    if (busy) return;
    error = "";
    const f = await open({
      directory: true,
      multiple: false,
      title: "选择用于解压的空文件夹",
    });
    if (f) importDest = f;
  }

  async function checkGit() {
    gitChecking = true;
    error = "";
    try {
      gitInfo = await api.gitCheck();
      gitEditingPath = !gitInfo.available;
      if (gitInfo.available) customGitPath = gitInfo.path;
    } catch (e) {
      gitInfo = { available: false, path: "", version: "" };
      gitEditingPath = true;
      error = String(e);
    } finally {
      gitChecking = false;
    }
  }

  async function verifyCustomGit() {
    if (!customGitPath.trim() || gitChecking) return;
    gitChecking = true;
    error = "";
    try {
      gitInfo = await api.gitCheck(customGitPath.trim());
      gitEditingPath = false;
    } catch (e) {
      error = String(e);
    } finally {
      gitChecking = false;
    }
  }

  async function pickGitExe() {
    if (busy || gitChecking) return;
    error = "";
    const f = await open({
      multiple: false,
      directory: false,
      title: "选择 git 可执行文件",
      ...(IS_WIN ? { filters: [{ name: "Git 可执行文件", extensions: ["exe"] }] } : {}),
    });
    if (!f) return;
    customGitPath = f;
    await verifyCustomGit();
  }

  function laterGitDefault() {
    clearTimeout(gitUrlTimer);
    const u = gitUrl.trim();
    if (!u) return;
    gitUrlTimer = setTimeout(async () => {
      try {
        const r = await api.gitDefaultDir(u);
        if (!gitTitleEdited) gitTitle = uniqueTitle(r.title, "远程知识库");
        if (!gitDestEdited) gitDest = r.dest;
      } catch {}
    }, 300);
  }

  async function pickGitDest() {
    if (busy) return;
    error = "";
    const f = await open({
      directory: true,
      multiple: false,
      title: "选择用于存放克隆仓库的空文件夹",
    });
    if (f) {
      gitDest = f;
      gitDestEdited = true;
    }
  }

  async function bind() {
    busy = true;
    error = "";
    progress = {
      label:
        kind === "zim"
          ? "正在读取 ZIM…"
          : kind === "git"
            ? "正在浅克隆远程仓库…"
            : kind === "import"
              ? pkgInfo?.kind === "smartdir" && importMode === "folder"
                ? "正在解压并绑定本地文件夹…"
                : "正在导入数据包…"
              : "正在绑定…",
      done: 0,
      total: 0,
    };
    try {
      if (kind === "notes") {
        onbound(await api.createNotesLibrary(notesTitle, libraryId));
        return;
      }
      if (kind === "git") {
        const st = await api.addGitLibrary(
          gitUrl,
          gitDest,
          gitTitle,
          gitBranch,
          exts,
          Math.max(0, Math.floor(Number(maxMb) || 0)),
          libraryId,
        );
        onbound(st);
        return;
      }
      if (kind === "import") {
        const libTitle = importTitle.trim() || null;
        if (pkgInfo.kind === "smartdir" && importMode === "folder") {
          const res = await api.importDirAsFolder(pkgPath, importDest, libTitle, importWritable, libraryId);
          onbound(res.status, { imported: true, title: res.title, count: res.notes, unit: "个文件" });
        } else {
          const res = await api.importPackage(pkgPath, null, null, libTitle, libraryId);
          onbound(res.status, { imported: true, title: res.title, count: res.notes, unit: "篇笔记" });
        }
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
      // giving up on a huge ZIM is a choice, not a failure
      error = String(e) === "已取消" ? "" : String(e);
    } finally {
      busy = false;
      progress = null;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-labelledby="bind-title">
    <header class="dialog-header"><h2 id="bind-title">{kind === "import" ? "导入知识库" : "添加知识库"}</h2></header>
    <div class="dialog-content">
    {#if !kind}
      {#each KIND_GROUPS as g}
        <div class="kind-group">
          <div class="kind-group-title muted">{g.title}</div>
          {#each g.items as k}
            <button class="kind" disabled={!k.ready} onclick={() => enterKind(k.id)}>
              <strong>{k.name}</strong>
              <span class="muted">{k.desc}</span>
            </button>
          {/each}
        </div>
      {/each}
    {:else if kind === "notes"}
      <p class="muted">新建一个空的知识库，之后在目录树里右键添加笔记。内容保存在本程序的数据目录里（一个 SQLite 文件）；它是你自己写的内容，没有原始数据可以重新导入，请注意备份。</p>
      <label class="field">名称 <input type="text" use:autofocus={{ select: true }} bind:value={notesTitle} placeholder="例如：读书笔记" /></label>
      <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder={ID_PLACEHOLDER} title={ID_TITLE} /></label>
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
        <label class="field">名称 <input type="text" use:autofocus={{ select: true }} bind:value={folderTitle} /></label>
        <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder={ID_PLACEHOLDER} title={ID_TITLE} /></label>
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
    {:else if kind === "git"}
      {#if gitChecking && !gitInfo}
        <p class="muted">正在检测本机 git 命令…</p>
      {:else if !gitInfo?.available || gitEditingPath}
        <p class="muted">
          {#if !gitInfo?.available}
            未检测到可用的 <code>git</code> 命令。请先安装 Git，或手动指定本机 <code>git</code> 可执行文件的位置，验证通过后才能继续。
          {:else}
            指定本机 <code>git</code> 可执行文件的位置：
          {/if}
        </p>
        <div class="row">
          <input type="text" use:autofocus bind:value={customGitPath} placeholder={IS_WIN ? "C:\\Program Files\\Git\\cmd\\git.exe" : "/opt/homebrew/bin/git"} aria-label="git 可执行文件路径" disabled={gitChecking} />
          <button type="button" onclick={pickGitExe} disabled={gitChecking}>浏览…</button>
          <button type="button" class="primary" onclick={verifyCustomGit} disabled={!customGitPath.trim() || gitChecking}>{gitChecking ? "验证中…" : "验证"}</button>
          {#if gitInfo?.available}
            <button type="button" onclick={() => { gitEditingPath = false; error = ""; }} disabled={gitChecking}>取消</button>
          {/if}
        </div>
        {#if error}<p class="error">{error}</p>{/if}
      {:else}
        <p class="muted">克隆远程 Git 仓库作为只读知识库（默认浅克隆 <code>--depth 1</code>），支持手动和定期检查更新。如需本地修改与同步，请在本地文件夹中手动操作。</p>
        <p class="muted small">
          Git 可用：{gitInfo.version}（<code>{gitInfo.path}</code>）
          <button type="button" class="link" onclick={() => { gitEditingPath = true; customGitPath = gitInfo.path; error = ""; }} disabled={busy}>更改</button>
        </p>
        <label class="field">
          仓库地址
          <input type="text" use:autofocus bind:value={gitUrl} oninput={laterGitDefault} placeholder="https://github.com/user/repo.git" disabled={busy} />
        </label>
        <label class="field">
          名称
          <input type="text" bind:value={gitTitle} oninput={() => (gitTitleEdited = true)} placeholder="留空默认使用仓库名" disabled={busy} />
        </label>
        <label class="field">
          知识库 ID（可选）
          <input type="text" bind:value={libraryId} placeholder={ID_PLACEHOLDER} title={ID_TITLE} disabled={busy} />
        </label>
        <label class="field">
          分支（可选）
          <input type="text" bind:value={gitBranch} placeholder="留空使用仓库默认分支" disabled={busy} />
        </label>
        <label class="field">
          本地存放位置
          <input type="text" bind:value={gitDest} oninput={() => (gitDestEdited = true)} placeholder="输入仓库地址后自动分配空目录" disabled={busy} />
          <button type="button" onclick={pickGitDest} disabled={busy}>更改…</button>
        </label>
        <p class="hint muted">已默认分配空目录，可直接克隆，也可点击「更改…」选择其他空文件夹。</p>
        <fieldset>
          <legend>索引哪些文件</legend>
          <label class="field">文件后缀（逗号或空格分隔）<input type="text" bind:value={exts} disabled={busy} /></label>
          <p class="hint muted">txt、md、html 等文本，以及 pdf、docx、pptx、odt 读取全文；其他后缀只索引文件名、类型、大小与日期。</p>
          <label class="field">单个文件最大 <input type="number" min="0" class="num" bind:value={maxMb} disabled={busy} /> MB（0 为不限制）</label>
        </fieldset>
        {#if error}<p class="error">{error}</p>{/if}
      {/if}
    {:else if kind === "import"}
      <p class="muted">选择导出的知识库数据包（<code>.smartlib</code> 或 <code>.smartdir</code>），选好后自动识别类型。</p>
      <div class="row">
        <input type="text" readonly value={pkgPath} placeholder="尚未选择数据包" aria-label="数据包文件" />
        <button type="button" onclick={pickPackage} disabled={busy}>选择数据包…</button>
      </div>
      {#if error}<p class="error">{error}</p>{/if}
      {#if pkgInfo}
        <p class="muted small">
          {#if pkgInfo.kind === "smartdir"}
            文件夹数据包（<code>.smartdir</code>），包含 {pkgInfo.files} 个文件。
          {:else}
            知识库数据包（<code>.smartlib</code>），将导入为新的默认知识库。
          {/if}
        </p>
        <label class="field">名称 <input type="text" use:autofocus={{ select: true }} bind:value={importTitle} placeholder={pkgInfo.title} disabled={busy} /></label>
        <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder={ID_PLACEHOLDER} title={ID_TITLE} disabled={busy} /></label>
        {#if pkgInfo.kind === "smartdir"}
          <fieldset>
            <legend>导入方式</legend>
            <label class="opt">
              <input type="radio" name="import-mode" value="folder" bind:group={importMode} disabled={busy} />
              导入为本地文件夹知识库
            </label>
            <p class="hint muted">原样解压到选定的空文件夹，并绑定为本地文件夹知识库。如需合并到已有文件夹，请解压到空目录后手动合并。</p>
            <label class="opt">
              <input type="radio" name="import-mode" value="notes" bind:group={importMode} disabled={busy} />
              导入为默认知识库
            </label>
            <p class="hint muted">转换为默认知识库（保留目录树、元信息、脑图、白板与图片附件）。</p>
          </fieldset>
          {#if importMode === "folder"}
            <label class="field">
              目标位置
              <input type="text" bind:value={importDest} placeholder="默认自动创建空目录" disabled={busy} />
              <button type="button" onclick={pickImportDest} disabled={busy}>更改…</button>
            </label>
            <p class="hint muted">已默认分配空目录，可直接导入，也可点击「更改…」选择其他空文件夹。</p>
            <label class="opt">
              <input type="checkbox" bind:checked={importWritable} disabled={busy} />
              绑定后允许编辑（可读写模式）
            </label>
          {/if}
        {/if}
      {/if}
    {:else if kind === "zim"}
      <p class="muted">选择 ZIM 文件，选好先检查一次文件；可以读取时「绑定」才可用。数据包只读，绑定后可搜索标题并建立全文索引。</p>
      <div class="row">
        <input type="text" use:autofocus bind:value={path} oninput={laterInspectZim} placeholder={examplePath("wikipedia.zim")} aria-label="ZIM 文件" />
        <button type="button" onclick={pickZim}>选择文件…</button>
      </div>
      {#if zimInfo}
        <p class="muted small">ZIM {zimInfo.major}.{zimInfo.minor} · {zimInfo.entries.toLocaleString()} 条 · {zimInfo.clusters.toLocaleString()} 个簇 · {sizeText(zimInfo.bytes)} · 可以绑定</p>
      {/if}
      {#if zimError}<p class="error">{zimError}</p>{/if}
      <label class="field">知识库 ID（可选）<input type="text" bind:value={libraryId} placeholder="例如 wikipedia（仅小写字母、数字、-、_，创建后不可改）" title={ID_TITLE} /></label>
      <p class="hint muted">跨维基站点链接按知识库 ID 查找，例如 wikipedia。</p>
      {#if error}<p class="error">{error}</p>{/if}
    {:else}
    <p class="muted">选择 CBETA 的 bookcase.zip（或已解压的目录），选好就自动读取。Smart Library 只读取数据，不会修改它。</p>
    <form class="row" onsubmit={(e) => { e.preventDefault(); clearTimeout(inspectTimer); inspect(); }}>
      <input type="text" use:autofocus bind:value={path} oninput={laterInspect} placeholder={examplePath("bookcase.zip")} aria-label="数据路径" />
      <button type="button" onclick={pickFile}>选择文件…</button>
      <button type="button" onclick={pickFolder}>选择文件夹…</button>
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
        {#if cbetaTree && !cbetaBound}<label class="field">CBETA 知识库 ID（可选）<input type="text" bind:value={libraryIds[cbetaTree]} placeholder={ID_PLACEHOLDER} title={ID_TITLE} /></label>{/if}
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
            {#if html[t.id] && !t.bound_as}<label class="field">{t.title} 的知识库 ID（可选）<input type="text" bind:value={libraryIds[t.id]} placeholder={ID_PLACEHOLDER} title={ID_TITLE} /></label>{/if}
          {/each}
        </fieldset>
      {/if}

      {#if otherTrees.length}
        <p class="muted small">没有可读内容的目录树：{otherTrees.map((t) => t.title).join("、")}</p>
      {/if}
    {/if}

    {/if}

    </div>
    {#if busy}
      <div class="progress-row" role="status" aria-live="polite">
        {#if progress?.total}
          <progress value={progress.done} max={progress.total}></progress>
          <span class="muted">{progress.label} {Math.floor((progress.done / progress.total) * 100)}%</span>
        {:else}
          <progress></progress>
          <span class="muted">{progress?.label ?? "请稍候…"}</span>
        {/if}
        {#if kind === "zim"}<button type="button" class="link" onclick={() => api.cancelBind()}>取消</button>{/if}
      </div>
    {/if}
    <div class="actions">
      {#if kind && !initialPath}<button onclick={() => { kind = ""; src = null; error = ""; }} disabled={busy}>上一步</button>{/if}
      {#if !kind}<button class="primary" onclick={() => enterKind("notes")}>不确定选哪个？从这里开始</button>{/if}
      <button onclick={onclose} disabled={busy}>取消</button>
      {#if kind === "notes"}
        <button class="primary" disabled={!notesTitle.trim() || busy} onclick={bind}>{busy ? "创建中…" : "创建"}</button>
      {:else if kind === "git"}
        <button class="primary" disabled={!gitInfo?.available || gitEditingPath || !gitUrl.trim() || !gitDest.trim() || !exts.trim() || busy} onclick={bind}>{busy ? "克隆中…" : "克隆并添加"}</button>
      {:else if kind === "import"}
        <button class="primary" disabled={!pkgInfo || (pkgInfo.kind === "smartdir" && importMode === "folder" && !importDest.trim()) || busy} onclick={bind}>{busy ? "导入中…" : "导入"}</button>
      {:else if kind === "folder"}
        <button class="primary" disabled={!folder || !!folder.conflict || busy || !exts.trim()} onclick={bind}>{busy ? "绑定中…" : "绑定"}</button>
      {:else if kind === "zim"}
        <button class="primary" disabled={!zimInfo || busy} onclick={bind}>{busy ? "绑定中…" : "绑定"}</button>
      {:else if kind}
        <button class="primary" disabled={!src || busy} onclick={bind}>{busy ? (src ? "绑定中…" : "读取中…") : "绑定"}</button>
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
  .kind-group { margin-top: 12px; }
  .kind-group:first-child { margin-top: 2px; }
  .kind-group-title { font-size: 12px; font-weight: 600; margin: 0 0 4px 2px; letter-spacing: 0.02em; }
  .kind { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%; text-align: left; margin: 6px 0; padding: 10px 14px; }
  .kind:disabled { opacity: 0.55; }
  .error { color: var(--hl); }
  .progress-row { display: flex; align-items: center; gap: 10px; padding: 10px 22px; border-top: 1px solid var(--line); font-size: 13px; flex: none; }
  .progress-row progress { flex: 1; height: 8px; min-width: 120px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
</style>
