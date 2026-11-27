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
        { id: "git", name: "远程知识库（Git）", desc: "克隆远程 Git 仓库或直接绑定本地已克隆的 Git 仓库为只读知识库，支持手动与定期检查更新（只读）", ready: true },
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
  let gitSource = $state(""); // remote URL or local repo path
  let gitTitle = $state("");
  let gitBranch = $state("");
  let gitDest = $state("");
  let gitDestEdited = $state(false);
  let gitTitleEdited = $state(false);
  let gitUrlTimer = null;
  let gitDestTimer = null;
  let gitDirInfo = $state(null); // { path, exists, is_empty, is_git_repo, remote_url, branch, title, conflict }
  let gitDirError = $state("");
  const gitReusingLocal = $derived(!!(gitDirInfo?.is_git_repo && gitDirInfo?.remote_url));
  const gitSourceIsLocal = $derived(isLocalPathInput(gitSource));

  function isGitRemoteUrl(s) {
    const v = (s || "").trim();
    return /^(https?|ssh|git|ftps?):\/\//i.test(v) || /^[a-zA-Z0-9._-]+@[^:/\s]+:.+/.test(v);
  }
  function isLocalPathInput(s) {
    const v = (s || "").trim();
    if (!v || isGitRemoteUrl(v)) return false;
    return /^[a-zA-Z]:[\\/]/.test(v) || /^\\\\/.test(v) || /^([/~]|\.\.?[\\/])/.test(v);
  }
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
  let exts = $state("txt, md, html, htm, pdf, docx, pptx, odt");
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
      clearTimeout(gitDestTimer);
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

  function onGitSourceInput() {
    clearTimeout(gitUrlTimer);
    clearTimeout(gitDestTimer);
    error = "";
    gitDirError = "";
    const v = gitSource.trim();
    if (!v) {
      if (gitReusingLocal) {
        gitDest = "";
        gitDestEdited = false;
      }
      gitDirInfo = null;
      return;
    }
    if (isLocalPathInput(v)) {
      gitDest = v;
      gitDestEdited = true;
      gitDirInfo = null;
      gitDestTimer = setTimeout(() => inspectGitDest(v, { fromSource: true }), 350);
      return;
    }
    if (gitReusingLocal) {
      gitDest = "";
      gitDestEdited = false;
      gitDirInfo = null;
    }
    gitUrlTimer = setTimeout(async () => {
      try {
        const r = await api.gitDefaultDir(v);
        if (gitSource.trim() !== v) return;
        if (!gitTitleEdited) gitTitle = uniqueTitle(r.title, "远程知识库");
        if (!gitDestEdited) gitDest = r.dest;
      } catch {}
    }, 300);
  }

  async function inspectGitDest(dir, { fromSource = false } = {}) {
    const d = (dir ?? gitDest).trim();
    gitDirInfo = null;
    gitDirError = "";
    if (!d) return;
    try {
      const info = await api.gitInspectDir(d);
      if ((fromSource ? gitSource.trim() : gitDest.trim()) !== d) return;
      gitDirInfo = info;
      if (info.is_git_repo && info.remote_url) {
        if (!gitTitleEdited && info.title) gitTitle = uniqueTitle(info.title, "远程知识库");
      } else if (fromSource) {
        gitDirError = info.is_git_repo
          ? "该 Git 仓库未配置远程地址（remote），请改用「本地文件夹」添加。"
          : "该目录不是有效的 Git 仓库根目录，请选择包含 .git 的文件夹。";
      } else if (info.exists && !info.is_empty) {
        gitDirError = info.is_git_repo
          ? "该 Git 仓库未配置远程地址（remote），无法绑定为远程知识库，请改用「本地文件夹」添加。"
          : "目标文件夹不为空且不是 Git 仓库根目录，请选择空目录或已克隆的 Git 仓库。";
      }
    } catch (e) {
      if ((fromSource ? gitSource.trim() : gitDest.trim()) === d) gitDirError = String(e);
    }
  }

  function laterInspectGitDest() {
    gitDestEdited = true;
    gitDirInfo = null;
    gitDirError = "";
    clearTimeout(gitDestTimer);
    const d = gitDest.trim();
    if (!d) return;
    gitDestTimer = setTimeout(() => inspectGitDest(d), 350);
  }

  async function pickLocalGitRepo() {
    if (busy) return;
    error = "";
    const f = await open({
      directory: true,
      multiple: false,
      title: "选择本地已克隆的 Git 仓库文件夹",
    });
    if (f) {
      clearTimeout(gitUrlTimer);
      clearTimeout(gitDestTimer);
      gitSource = f;
      gitDest = f;
      gitDestEdited = true;
      await inspectGitDest(f, { fromSource: true });
    }
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
      clearTimeout(gitDestTimer);
      gitDest = f;
      gitDestEdited = true;
      await inspectGitDest(f);
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
            ? gitReusingLocal
              ? "正在绑定本地 Git 仓库…"
              : "正在浅克隆远程仓库…"
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
        const url = gitSourceIsLocal ? (gitDirInfo?.remote_url || "") : gitSource.trim();
        const dest = gitSourceIsLocal ? gitSource.trim() : gitDest.trim();
        const st = await api.addGitLibrary(
          url,
          dest,
          gitTitle,
          gitReusingLocal ? "" : gitBranch,
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
      <div class="gh-form">
        <p class="gh-intro muted">
          新建一个空的知识库，之后在目录树里右键添加笔记。内容保存在程序数据目录（SQLite 文件）中，请注意备份。
        </p>

        <section class="gh-step">
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={!!notesTitle.trim()}>1</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">基本信息</h3>

            <div class="gh-field">
              <label for="notes-title">名称 <span class="req" aria-hidden="true">*</span></label>
              <input id="notes-title" type="text" use:autofocus={{ select: true }} bind:value={notesTitle} placeholder="例如：读书笔记" disabled={busy} />
            </div>
          </div>
        </section>

        <section class="gh-step gh-step-last">
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge">2</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">知识库标识</h3>

            <div class="gh-card">
              <div class="gh-row">
                <div class="gh-row-info">
                  <label class="gh-row-title" for="notes-lib-id">知识库 ID</label>
                  <p class="gh-row-desc muted">固定 ID 便于跨库引用与长期维护，留空自动生成（创建后不可改）</p>
                </div>
                <div class="gh-row-ctrl">
                  <input id="notes-lib-id" type="text" class="gh-ctrl-sm" bind:value={libraryId} placeholder="留空自动生成" title={ID_TITLE} disabled={busy} />
                </div>
              </div>
            </div>
            {#if error}<p class="field-msg error">{error}</p>{/if}
          </div>
        </section>
      </div>
    {:else if kind === "folder"}
      <div class="gh-form">
        <p class="gh-intro muted">
          链接本机文件夹，按真实目录结构展示；默认只读，也可开启读写模式直接编辑笔记。
        </p>

        <section class="gh-step" class:gh-step-last={!folder || !!folder.conflict}>
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={folder && !folder.conflict}>1</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">基本信息</h3>

            <div class="gh-field">
              <label for="folder-path">文件夹 <span class="req" aria-hidden="true">*</span></label>
              <div class="input-with-btn">
                <input id="folder-path" type="text" readonly value={folder ? folder.path : ""} placeholder="尚未选择文件夹" />
                <button type="button" onclick={pickLibFolder} disabled={busy}>选择文件夹…</button>
              </div>
              {#if folder?.conflict}<p class="field-msg error">{folder.conflict}</p>{/if}
              {#if error}<p class="field-msg error">{error}</p>{/if}
            </div>

            {#if folder && !folder.conflict}
              <div class="gh-grid-2">
                <div class="gh-field">
                  <label for="folder-title">名称</label>
                  <input id="folder-title" type="text" use:autofocus={{ select: true }} bind:value={folderTitle} disabled={busy} />
                </div>
                <div class="gh-field">
                  <label for="folder-lib-id">知识库 ID</label>
                  <input id="folder-lib-id" type="text" bind:value={libraryId} placeholder="留空自动生成" title={ID_TITLE} disabled={busy} />
                </div>
              </div>
            {/if}
          </div>
        </section>

        {#if folder && !folder.conflict}
          <section class="gh-step gh-step-last">
            <div class="gh-rail" aria-hidden="true">
              <span class="gh-badge">2</span>
              <span class="gh-line"></span>
            </div>
            <div class="gh-step-body">
              <h3 class="gh-step-title">收录与权限配置</h3>

              <div class="gh-card">
                <div class="gh-row">
                  <div class="gh-row-info">
                    <label class="gh-row-title" for="folder-exts">索引文件后缀</label>
                    <p class="gh-row-desc muted">用于目录显示和全文索引，绑定后可在菜单修改</p>
                  </div>
                  <div class="gh-row-ctrl gh-ctrl-med">
                    <input id="folder-exts" type="text" bind:value={exts} placeholder="txt, md, html, pdf..." disabled={busy} />
                  </div>
                </div>
                <div class="gh-row">
                  <div class="gh-row-info">
                    <label class="gh-row-title" for="folder-max-mb">单个文件上限</label>
                    <p class="gh-row-desc muted">扫描版 PDF 等仅收录文件信息（0 为不限制）</p>
                  </div>
                  <div class="gh-row-ctrl gh-ctrl-inline">
                    <input id="folder-max-mb" type="number" min="0" class="num" bind:value={maxMb} disabled={busy} />
                    <span class="muted">MB</span>
                  </div>
                </div>
              </div>

              <div class="gh-card">
                <div class="gh-row">
                  <div class="gh-row-info">
                    <label class="gh-row-title" for="folder-writable">允许编辑笔记</label>
                    <p class="gh-row-desc muted">
                      {writable
                        ? "点「绑定」时会先说明规则和风险并需确认，之后可随时切换（.md、脑图、白板）。"
                        : "关闭则保持只读：不会新建、修改或删除文件夹中的任何文件。"}
                    </p>
                  </div>
                  <div class="gh-row-ctrl">
                    <label class="gh-switch">
                      <span class="muted">{writable ? "开启" : "只读"}</span>
                      <input id="folder-writable" type="checkbox" bind:checked={writable} disabled={busy} />
                      <span class="gh-switch-track" aria-hidden="true"></span>
                    </label>
                  </div>
                </div>
              </div>
            </div>
          </section>
        {/if}
      </div>
    {:else if kind === "git"}
      <div class="gh-form">
        <p class="gh-intro muted">
          克隆远程 Git 仓库或绑定本地已有仓库为只读知识库，支持定期检查更新。
        </p>

        <section class="gh-step" class:gh-step-last={!gitInfo?.available || gitEditingPath}>
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={gitInfo?.available && !gitEditingPath}>0</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">Git 环境</h3>

            {#if gitChecking && !gitInfo}
              <div class="gh-card">
                <div class="gh-row gh-row-slim">
                  <span class="muted small">正在检测本机 git 命令…</span>
                </div>
              </div>
            {:else if !gitInfo?.available || gitEditingPath}
              <div class="gh-card">
                <div class="gh-card-pad">
                  <p class="gh-row-desc muted">
                    {#if !gitInfo?.available}
                      未检测到可用的 <code>git</code> 命令。请先安装 Git，或手动指定 <code>git</code> 可执行文件位置，验证通过后才能继续。
                    {:else}
                      指定本机 <code>git</code> 可执行文件位置：
                    {/if}
                  </p>
                  <div class="input-with-btn">
                    <input type="text" use:autofocus bind:value={customGitPath} placeholder={IS_WIN ? "C:\\Program Files\\Git\\cmd\\git.exe" : "/opt/homebrew/bin/git"} aria-label="git 可执行文件路径" disabled={gitChecking} />
                    <button type="button" onclick={pickGitExe} disabled={gitChecking}>浏览…</button>
                    <button type="button" class="primary" onclick={verifyCustomGit} disabled={!customGitPath.trim() || gitChecking}>{gitChecking ? "验证中…" : "验证"}</button>
                    {#if gitInfo?.available}
                      <button type="button" onclick={() => { gitEditingPath = false; error = ""; }} disabled={gitChecking}>取消</button>
                    {/if}
                  </div>
                  {#if error}<p class="field-msg error">{error}</p>{/if}
                </div>
              </div>
            {:else}
              <div class="gh-card">
                <div class="gh-row gh-row-slim">
                  <div class="gh-env-status">
                    <span class="env-dot" aria-hidden="true"></span>
                    <span class="gh-env-ver">{gitInfo.version}</span>
                    <span class="gh-env-path muted" title={gitInfo.path}>{gitInfo.path}</span>
                  </div>
                  <button type="button" class="link small" onclick={() => { gitEditingPath = true; customGitPath = gitInfo.path; error = ""; }} disabled={busy}>更改路径</button>
                </div>
              </div>
            {/if}
          </div>
        </section>

        {#if gitInfo?.available && !gitEditingPath}
          <section class="gh-step">
            <div class="gh-rail" aria-hidden="true">
              <span class="gh-badge">1</span>
              <span class="gh-line"></span>
            </div>
            <div class="gh-step-body">
              <h3 class="gh-step-title">基本信息</h3>

              <div class="gh-field">
                <label for="git-source">仓库地址 <span class="req" aria-hidden="true">*</span></label>
                <div class="input-with-btn">
                  <input
                    id="git-source"
                    type="text"
                    use:autofocus
                    bind:value={gitSource}
                    oninput={onGitSourceInput}
                    placeholder="https://github.com/user/repo.git 或本地仓库路径"
                    disabled={busy}
                  />
                  <button type="button" onclick={pickLocalGitRepo} disabled={busy}>本地仓库…</button>
                </div>
                {#if gitDirInfo?.conflict}
                  <p class="field-msg error">{gitDirInfo.conflict}</p>
                {:else if gitDirError}
                  <p class="field-msg error">{gitDirError}</p>
                {:else if gitReusingLocal}
                  <p class="field-msg ok">
                    已识别本地仓库：{gitDirInfo.remote_url}{gitDirInfo.branch ? `（${gitDirInfo.branch}）` : ""}，将直接绑定
                  </p>
                {/if}
              </div>

              <div class="gh-grid-2">
                <div class="gh-field">
                  <label for="git-title">名称</label>
                  <input id="git-title" type="text" bind:value={gitTitle} oninput={() => (gitTitleEdited = true)} placeholder="留空自动使用仓库名" disabled={busy} />
                </div>
                <div class="gh-field">
                  <label for="git-lib-id">知识库 ID</label>
                  <input id="git-lib-id" type="text" bind:value={libraryId} placeholder="留空自动生成" title={ID_TITLE} disabled={busy} />
                </div>
              </div>
            </div>
          </section>

          <section class="gh-step gh-step-last">
            <div class="gh-rail" aria-hidden="true">
              <span class="gh-badge">2</span>
              <span class="gh-line"></span>
            </div>
            <div class="gh-step-body">
              <h3 class="gh-step-title">存储与索引配置</h3>

              {#if !gitSourceIsLocal}
                <div class="gh-card">
                  <div class="gh-row">
                    <div class="gh-row-info">
                      <label class="gh-row-title" for="git-dest">本地存放位置</label>
                      <p class="gh-row-desc muted">输入仓库地址后自动分配空目录</p>
                    </div>
                    <div class="gh-row-ctrl gh-ctrl-wide">
                      <div class="input-with-btn">
                        <input id="git-dest" type="text" bind:value={gitDest} oninput={laterInspectGitDest} placeholder="自动分配" disabled={busy} />
                        <button type="button" onclick={pickGitDest} disabled={busy}>更改…</button>
                      </div>
                    </div>
                  </div>
                  {#if !gitReusingLocal}
                    <div class="gh-row">
                      <div class="gh-row-info">
                        <label class="gh-row-title" for="git-branch">拉取分支</label>
                        <p class="gh-row-desc muted">留空使用仓库默认分支</p>
                      </div>
                      <div class="gh-row-ctrl">
                        <input id="git-branch" type="text" class="gh-ctrl-sm" bind:value={gitBranch} placeholder="默认分支" disabled={busy} />
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              <div class="gh-card">
                <div class="gh-row">
                  <div class="gh-row-info">
                    <label class="gh-row-title" for="git-exts">索引文件后缀</label>
                    <p class="gh-row-desc muted">文本及 pdf/docx/pptx/odt 读全文，其余仅索引文件信息</p>
                  </div>
                  <div class="gh-row-ctrl gh-ctrl-med">
                    <input id="git-exts" type="text" bind:value={exts} placeholder="txt, md, html, pdf..." disabled={busy} />
                  </div>
                </div>
                <div class="gh-row">
                  <div class="gh-row-info">
                    <label class="gh-row-title" for="git-max-mb">单个文件上限</label>
                    <p class="gh-row-desc muted">超过大小跳过正文索引（0 为不限制）</p>
                  </div>
                  <div class="gh-row-ctrl gh-ctrl-inline">
                    <input id="git-max-mb" type="number" min="0" class="num" bind:value={maxMb} disabled={busy} />
                    <span class="muted">MB</span>
                  </div>
                </div>
              </div>
            </div>
          </section>

          {#if error}<p class="error">{error}</p>{/if}
        {/if}
      </div>
    {:else if kind === "import"}
      <div class="gh-form">
        <p class="gh-intro muted">
          选择导出的知识库数据包（<code>.smartlib</code> 或 <code>.smartdir</code>），选好后自动识别类型。
        </p>

        <section class="gh-step" class:gh-step-last={!pkgInfo || pkgInfo.kind !== "smartdir"}>
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={!!pkgInfo}>1</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">选择数据包</h3>

            <div class="gh-field">
              <label for="pkg-path">数据包文件 <span class="req" aria-hidden="true">*</span></label>
              <div class="input-with-btn">
                <input id="pkg-path" type="text" readonly value={pkgPath} placeholder="尚未选择数据包" />
                <button type="button" onclick={pickPackage} disabled={busy}>选择数据包…</button>
              </div>
              {#if error}
                <p class="field-msg error">{error}</p>
              {:else if pkgInfo}
                <p class="field-msg ok">
                  {#if pkgInfo.kind === "smartdir"}
                    已识别文件夹数据包（.smartdir），包含 {pkgInfo.files} 个文件
                  {:else}
                    已识别知识库数据包（.smartlib），将导入为新的默认知识库
                  {/if}
                </p>
              {/if}
            </div>

            {#if pkgInfo}
              <div class="gh-grid-2">
                <div class="gh-field">
                  <label for="import-title">名称</label>
                  <input id="import-title" type="text" use:autofocus={{ select: true }} bind:value={importTitle} placeholder={pkgInfo.title} disabled={busy} />
                </div>
                <div class="gh-field">
                  <label for="import-lib-id">知识库 ID</label>
                  <input id="import-lib-id" type="text" bind:value={libraryId} placeholder="留空自动生成" title={ID_TITLE} disabled={busy} />
                </div>
              </div>
            {/if}
          </div>
        </section>

        {#if pkgInfo?.kind === "smartdir"}
          <section class="gh-step gh-step-last">
            <div class="gh-rail" aria-hidden="true">
              <span class="gh-badge">2</span>
              <span class="gh-line"></span>
            </div>
            <div class="gh-step-body">
              <h3 class="gh-step-title">导入配置</h3>

              <div class="gh-card">
                <div class="gh-row">
                  <div class="gh-row-info">
                    <span class="gh-row-title">导入方式</span>
                    <p class="gh-row-desc muted">
                      {importMode === "folder"
                        ? "原样解压到空文件夹并绑定为本地文件夹知识库。"
                        : "转换为默认知识库（保留目录树、元信息、脑图、白板与图片附件）。"}
                    </p>
                  </div>
                  <div class="gh-row-ctrl">
                    <div class="gh-seg" role="radiogroup" aria-label="导入方式">
                      <button type="button" role="radio" aria-checked={importMode === "folder"} class:active={importMode === "folder"} onclick={() => (importMode = "folder")} disabled={busy}>本地文件夹</button>
                      <button type="button" role="radio" aria-checked={importMode === "notes"} class:active={importMode === "notes"} onclick={() => (importMode = "notes")} disabled={busy}>默认知识库</button>
                    </div>
                  </div>
                </div>
              </div>

              {#if importMode === "folder"}
                <div class="gh-card">
                  <div class="gh-row">
                    <div class="gh-row-info">
                      <label class="gh-row-title" for="import-dest">解压目标位置</label>
                      <p class="gh-row-desc muted">已默认分配空目录，也可选择其他空文件夹</p>
                    </div>
                    <div class="gh-row-ctrl gh-ctrl-wide">
                      <div class="input-with-btn">
                        <input id="import-dest" type="text" bind:value={importDest} placeholder="默认自动创建空目录" disabled={busy} />
                        <button type="button" onclick={pickImportDest} disabled={busy}>更改…</button>
                      </div>
                    </div>
                  </div>
                  <div class="gh-row">
                    <div class="gh-row-info">
                      <label class="gh-row-title" for="import-writable">允许编辑笔记</label>
                      <p class="gh-row-desc muted">
                        {importWritable ? "绑定后可直接编辑其中的笔记（可读写模式）。" : "关闭则以只读方式绑定。"}
                      </p>
                    </div>
                    <div class="gh-row-ctrl">
                      <label class="gh-switch">
                        <span class="muted">{importWritable ? "开启" : "只读"}</span>
                        <input id="import-writable" type="checkbox" bind:checked={importWritable} disabled={busy} />
                        <span class="gh-switch-track" aria-hidden="true"></span>
                      </label>
                    </div>
                  </div>
                </div>
              {/if}
            </div>
          </section>
        {/if}
      </div>
    {:else if kind === "zim"}
      <div class="gh-form">
        <p class="gh-intro muted">
          选择只读的 ZIM 离线数据包，检查通过后即可绑定，支持标题搜索与全文索引。
        </p>

        <section class="gh-step">
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={!!zimInfo}>1</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">选择 ZIM 文件</h3>

            <div class="gh-field">
              <label for="zim-path">文件路径 <span class="req" aria-hidden="true">*</span></label>
              <div class="input-with-btn">
                <input id="zim-path" type="text" use:autofocus bind:value={path} oninput={laterInspectZim} placeholder={examplePath("wikipedia.zim")} disabled={busy} />
                <button type="button" onclick={pickZim} disabled={busy}>选择文件…</button>
              </div>
              {#if zimInfo}
                <p class="field-msg ok">ZIM {zimInfo.major}.{zimInfo.minor} · {zimInfo.entries.toLocaleString()} 条 · {zimInfo.clusters.toLocaleString()} 个簇 · {sizeText(zimInfo.bytes)} · 可以绑定</p>
              {/if}
              {#if zimError}<p class="field-msg error">{zimError}</p>{/if}
            </div>
          </div>
        </section>

        <section class="gh-step gh-step-last">
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge">2</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">知识库标识</h3>

            <div class="gh-card">
              <div class="gh-row">
                <div class="gh-row-info">
                  <label class="gh-row-title" for="zim-lib-id">知识库 ID</label>
                  <p class="gh-row-desc muted">跨维基站点链接按此 ID 查找，例如 wikipedia（创建后不可改）</p>
                </div>
                <div class="gh-row-ctrl">
                  <input id="zim-lib-id" type="text" class="gh-ctrl-sm" bind:value={libraryId} placeholder="例如 wikipedia" title={ID_TITLE} disabled={busy} />
                </div>
              </div>
            </div>
            {#if error}<p class="field-msg error">{error}</p>{/if}
          </div>
        </section>
      </div>
    {:else}
      <div class="gh-form">
        <p class="gh-intro muted">
          选择 CBETA 的 <code>bookcase.zip</code> 或已解压目录，选好后自动读取。Smart Library 只读取数据，不会修改它。
        </p>

        <section class="gh-step" class:gh-step-last={!src}>
          <div class="gh-rail" aria-hidden="true">
            <span class="gh-badge" class:ok={!!src}>1</span>
            <span class="gh-line"></span>
          </div>
          <div class="gh-step-body">
            <h3 class="gh-step-title">选择数据源</h3>

            <form class="gh-field" onsubmit={(e) => { e.preventDefault(); clearTimeout(inspectTimer); inspect(); }}>
              <label for="cbeta-path">数据路径 <span class="req" aria-hidden="true">*</span></label>
              <div class="input-with-btn">
                <input id="cbeta-path" type="text" use:autofocus bind:value={path} oninput={laterInspect} placeholder={examplePath("bookcase.zip")} disabled={busy} />
                <button type="button" onclick={pickFile} disabled={busy}>选择文件…</button>
                <button type="button" onclick={pickFolder} disabled={busy}>选择文件夹…</button>
              </div>
              {#if error && kind === "cbeta"}<p class="field-msg error">{error}</p>{/if}
              {#if src && kind === "cbeta"}
                <p class="field-msg ok">已识别 CBETA {src.data_version}（{src.version}）· {src.path}</p>
              {/if}
            </form>
          </div>
        </section>

        {#if src && kind === "cbeta"}
          <section class="gh-step gh-step-last">
            <div class="gh-rail" aria-hidden="true">
              <span class="gh-badge">2</span>
              <span class="gh-line"></span>
            </div>
            <div class="gh-step-body">
              <h3 class="gh-step-title">选择目录树</h3>

              {#if cbetaTrees.length}
                <div class="gh-card">
                  <div class="gh-card-head">
                    <span class="gh-row-title">CBETA 藏经（只能选一棵目录树）</span>
                    <p class="gh-row-desc muted">这几棵目录树显示同一份数据，绑定后如需更换请先解除绑定。</p>
                  </div>
                  {#each cbetaTrees as t}
                    <label class="gh-check-row" class:disabled={cbetaBound}>
                      <input type="radio" name="cbeta" value={t.id} bind:group={cbetaTree} disabled={cbetaBound || busy} />
                      <span class="gh-check-label">
                        {t.title}
                        <span class="muted">· {t.leaves} 个条目{t.bound_as ? " · 当前绑定" : ""}</span>
                      </span>
                    </label>
                  {/each}
                  {#if !cbetaBound}
                    <label class="gh-check-row">
                      <input type="radio" name="cbeta" value="" bind:group={cbetaTree} disabled={busy} />
                      <span class="gh-check-label muted">不绑定 CBETA</span>
                    </label>
                  {/if}
                  {#if cbetaTree && !cbetaBound}
                    <div class="gh-row">
                      <div class="gh-row-info">
                        <label class="gh-row-title" for="cbeta-lib-id">CBETA 知识库 ID</label>
                        <p class="gh-row-desc muted">留空自动生成，创建后不可改</p>
                      </div>
                      <div class="gh-row-ctrl">
                        <input id="cbeta-lib-id" type="text" class="gh-ctrl-sm" bind:value={libraryIds[cbetaTree]} placeholder="留空自动生成" title={ID_TITLE} disabled={busy} />
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              {#if htmlTrees.length}
                <div class="gh-card">
                  <div class="gh-card-head">
                    <span class="gh-row-title">其他知识库（可同时绑定）</span>
                    <p class="gh-row-desc muted">数据各不相同，每个知识库拥有独立索引。</p>
                  </div>
                  {#each htmlTrees as t}
                    <div class="gh-check-row" class:disabled={!!t.bound_as}>
                      <label class="gh-check-main">
                        <input type="checkbox" bind:checked={html[t.id]} disabled={!!t.bound_as || busy} />
                        <span class="gh-check-label">
                          {t.title}
                          <span class="muted">· a/{t.code}/ · {t.leaves} 篇{t.bound_as ? " · 已绑定" : ""}</span>
                        </span>
                      </label>
                      {#if html[t.id] && !t.bound_as}
                        <input
                          type="text"
                          class="gh-ctrl-sm"
                          bind:value={libraryIds[t.id]}
                          placeholder="ID（留空自动）"
                          title={ID_TITLE}
                          aria-label="{t.title} 的知识库 ID"
                          disabled={busy}
                        />
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}

              {#if otherTrees.length}
                <p class="field-msg muted">没有可读内容的目录树：{otherTrees.map((t) => t.title).join("、")}</p>
              {/if}
            </div>
          </section>
        {/if}
      </div>
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
        <button
          class="primary"
          disabled={!gitInfo?.available || gitEditingPath || (gitSourceIsLocal ? !gitReusingLocal : !gitSource.trim() || !gitDest.trim()) || !!gitDirInfo?.conflict || !!gitDirError || !exts.trim() || busy}
          onclick={bind}
        >
          {busy ? (gitSourceIsLocal || gitReusingLocal ? "绑定中…" : "克隆中…") : gitSourceIsLocal || gitReusingLocal ? "直接绑定" : "克隆并添加"}
        </button>
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
  .small { font-size: 13px; }
  .kind-group { margin-top: 12px; }
  .kind-group:first-child { margin-top: 2px; }
  .kind-group-title { font-size: 12px; font-weight: 600; margin: 0 0 4px 2px; letter-spacing: 0.02em; }
  .kind { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%; text-align: left; margin: 6px 0; padding: 10px 14px; }
  .kind:disabled { opacity: 0.55; }
  .error { color: var(--hl); }
  .progress-row { display: flex; align-items: center; gap: 10px; padding: 10px 22px; border-top: 1px solid var(--line); font-size: 13px; flex: none; }
  .progress-row progress { flex: 1; height: 8px; min-width: 120px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }

  /* ---- GitHub-style stepped card forms ---- */
  .gh-form { display: flex; flex-direction: column; gap: 10px; }
  .gh-intro { margin: 0 0 6px; font-size: 13px; line-height: 1.5; }

  .gh-step { display: flex; gap: 14px; }
  .gh-rail {
    display: flex; flex-direction: column; align-items: center;
    flex: none; width: 24px;
  }
  .gh-badge {
    width: 24px; height: 24px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; font-weight: 600;
    background: var(--panel); color: var(--muted);
    border: 1px solid var(--line);
    flex: none;
  }
  .gh-badge.ok {
    color: var(--note);
    border-color: color-mix(in srgb, var(--note) 45%, var(--line));
    background: color-mix(in srgb, var(--note) 10%, var(--paper));
  }
  .gh-line {
    width: 1px; flex: 1; margin: 6px 0 2px;
    background: var(--line);
  }
  .gh-step-last .gh-line { opacity: 0.55; }

  .gh-step-body {
    flex: 1; min-width: 0;
    display: flex; flex-direction: column; gap: 12px;
    padding-bottom: 14px;
  }
  .gh-step-last .gh-step-body { padding-bottom: 4px; }
  .gh-step-title {
    margin: 2px 0 0; font-size: 14px; font-weight: 600;
    line-height: 20px; color: var(--ink);
  }

  .gh-field { display: flex; flex-direction: column; gap: 5px; }
  .gh-field > label { font-size: 13px; font-weight: 600; color: var(--ink); }
  .req { color: var(--hl); margin-left: 2px; }
  .gh-grid-2 { display: grid; grid-template-columns: 1.25fr 1fr; gap: 12px; }

  .gh-form input[type="text"],
  .gh-form input[type="number"] {
    width: 100%;
    padding: 6px 10px;
    border: 1px solid color-mix(in srgb, var(--line) 75%, var(--muted) 25%);
    border-radius: 6px;
    background: var(--bg);
    color: var(--ink);
    font-size: 13px;
  }
  .gh-form input[type="number"].num { width: 78px; padding: 5px 8px; text-align: right; }
  .gh-form input::placeholder { color: color-mix(in srgb, var(--muted) 62%, transparent); }

  .input-with-btn { display: flex; gap: 8px; align-items: center; }
  .input-with-btn input { flex: 1; min-width: 0; }
  .input-with-btn button { flex: none; padding: 5px 11px; font-size: 13px; }

  .field-msg { margin: 2px 0 0; font-size: 12px; line-height: 1.45; }
  .field-msg.ok { color: var(--note); }

  .gh-card {
    border: 1px solid var(--line);
    border-radius: 8px;
    background: color-mix(in srgb, var(--panel) 35%, var(--paper));
    overflow: hidden;
  }
  .gh-row {
    display: flex; align-items: center; justify-content: space-between; gap: 16px;
    padding: 10px 14px;
  }
  .gh-row-slim { padding: 8px 14px; }
  .gh-card-pad { display: flex; flex-direction: column; gap: 8px; padding: 10px 14px; }
  .gh-row + .gh-row { border-top: 1px solid var(--line); }
  .gh-row-info { flex: 1; min-width: 0; }
  .gh-row-title { display: block; font-size: 13px; font-weight: 600; color: var(--ink); line-height: 1.35; }
  .gh-row-desc { margin: 2px 0 0; font-size: 12px; line-height: 1.4; }
  .gh-row-ctrl { flex: none; }
  .gh-ctrl-wide { width: 290px; }
  .gh-ctrl-med { width: 240px; }
  .gh-ctrl-sm { width: 160px; }
  .gh-ctrl-inline { display: flex; align-items: center; gap: 6px; font-size: 13px; }

  .gh-env-status {
    display: flex; align-items: center; gap: 8px;
    min-width: 0; font-size: 13px;
  }
  .gh-env-ver { font-weight: 500; color: var(--ink); flex: none; }
  .gh-env-path {
    font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .env-dot {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--note); flex: none;
  }

  .gh-switch {
    display: inline-flex; align-items: center; gap: 8px;
    cursor: pointer; font-size: 12px; user-select: none;
  }
  .gh-switch input {
    position: absolute; opacity: 0; width: 0; height: 0; pointer-events: none;
  }
  .gh-switch-track {
    position: relative; width: 34px; height: 20px; border-radius: 999px;
    background: var(--panel); border: 1px solid color-mix(in srgb, var(--line) 70%, var(--muted) 30%);
    transition: background 0.15s, border-color 0.15s;
  }
  .gh-switch-track::after {
    content: ""; position: absolute; top: 2px; left: 2px;
    width: 14px; height: 14px; border-radius: 50%;
    background: var(--muted); transition: transform 0.15s, background 0.15s;
  }
  .gh-switch input:checked + .gh-switch-track {
    background: var(--accent); border-color: var(--accent);
  }
  .gh-switch input:checked + .gh-switch-track::after {
    transform: translateX(14px); background: var(--paper);
  }
  .gh-switch input:focus-visible + .gh-switch-track {
    outline: 2px solid var(--accent); outline-offset: 2px;
  }

  .gh-seg {
    display: inline-flex; padding: 2px; border-radius: 7px;
    background: var(--panel); border: 1px solid var(--line);
  }
  .gh-seg button {
    border: none; background: transparent; border-radius: 5px;
    padding: 4px 10px; font-size: 12px; color: var(--muted);
    transition: background 0.12s, color 0.12s;
  }
  .gh-seg button:hover { color: var(--ink); }
  .gh-seg button.active {
    background: var(--paper); color: var(--ink); font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .gh-card-head {
    padding: 10px 14px 8px;
    background: color-mix(in srgb, var(--panel) 65%, var(--paper));
  }
  .gh-check-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 8px 14px; border-top: 1px solid var(--line);
    font-size: 13px; cursor: pointer;
  }
  .gh-check-row.disabled { opacity: 0.65; cursor: default; }
  .gh-check-main {
    display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; cursor: inherit;
  }
  .gh-check-label { flex: 1; min-width: 0; }
  .gh-check-row > input[type="radio"] { flex: none; margin: 0; }
  .gh-check-main > input[type="checkbox"] { flex: none; margin: 0; }
</style>
