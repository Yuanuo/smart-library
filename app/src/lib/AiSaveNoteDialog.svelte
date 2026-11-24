<script>
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import { settings } from "./settings.svelte.js";
  import { autofocus } from "./autofocus.js";

  let {
    kind = "md", // "md" | "mindmap"
    initialTitle = "",
    generatingTitle = false,
    onConfirm,
    onClose,
  } = $props();

  let libs = $state([]);
  let selectedLib = $state(settings.aiSaveLib || "");
  let path = $state(settings.aiSavePath || "");
  let title = $state("");
  let userEditedTitle = $state(false);
  let rememberDefault = $state(false);
  let pathSuggestions = $state([]);
  let saving = $state(false);
  let err = $state("");

  $effect(() => {
    if (!userEditedTitle && initialTitle) {
      title = initialTitle;
    }
  });

  async function loadPaths(libId) {
    const lib = libs.find((l) => l.id === libId);
    if (!lib) {
      pathSuggestions = [];
      return;
    }
    try {
      if (lib.kind === "folder") {
        pathSuggestions = (await api.folderDirs(lib.id)) || [];
      } else if (lib.kind === "notes") {
        const targets = (await api.noteTargets("")) || [];
        const prefix = `${lib.title}/`;
        pathSuggestions = targets
          .filter((t) => t.lib === lib.id || String(t.path || "").startsWith(prefix))
          .map((t) => {
            const p = String(t.path || "");
            return p.startsWith(prefix) ? p.slice(prefix.length) : p;
          })
          .filter(Boolean);
      }
    } catch {
      pathSuggestions = [];
    }
  }

  onMount(async () => {
    try {
      const st = await api.status();
      libs = (st.libraries || []).filter(
        (l) => l.kind === "notes" || (l.kind === "folder" && l.writable),
      );
      if (libs.length && !libs.some((l) => l.id === selectedLib)) {
        selectedLib = libs[0].id;
      }
      if (selectedLib) loadPaths(selectedLib);
    } catch (e) {
      err = String(e);
    }
  });

  async function submit(e) {
    e?.preventDefault?.();
    const cleanTitle = title.trim();
    if (!cleanTitle || saving) return;
    saving = true;
    err = "";
    try {
      await onConfirm({
        lib: selectedLib || null,
        libKind: libs.find((l) => l.id === selectedLib)?.kind || "notes",
        path: path.trim(),
        title: cleanTitle,
        rememberDefault,
      });
      onClose?.();
    } catch (ex) {
      err = String(ex);
      saving = false;
    }
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onmousedown={(e) => e.target === e.currentTarget && onClose?.()}>
  <div
    class="dialog dialog-shell"
    role="dialog"
    aria-modal="true"
    aria-label={kind === "mindmap" ? "保存为新脑图" : "保存为新笔记"}
    tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onClose?.()}>
    <form style="display:contents" onsubmit={submit}>
    <header class="dialog-header">
      <h2>{kind === "mindmap" ? "保存为新脑图" : "保存为新笔记"}</h2>
    </header>
    <div class="dialog-content">
      <label>
        标题
        <input
          type="text"
          use:autofocus={{ select: true }}
          value={title}
          placeholder={generatingTitle ? "AI 正在生成标题…" : "输入文档标题"}
          oninput={(e) => {
            userEditedTitle = true;
            title = e.currentTarget.value;
          }} />
      </label>
      {#if generatingTitle && !userEditedTitle}
        <p class="muted hint">正在由 AI 生成简短标题…</p>
      {/if}

      <label>
        知识库
        {#if libs.length > 0}
          <select
            value={selectedLib}
            onchange={(e) => {
              selectedLib = e.currentTarget.value;
              loadPaths(selectedLib);
            }}>
            {#each libs as l (l.id)}
              <option value={l.id}>{l.title}（{l.kind === "folder" ? "本地文件夹" : "内部笔记"}）</option>
            {/each}
          </select>
        {:else}
          <span class="muted">暂无可写库，将自动创建「我的笔记」库</span>
        {/if}
      </label>

      <label>
        存放位置
        <input
          type="text"
          list="ai-save-paths"
          bind:value={path}
          placeholder="留空保存在根目录；可填路径如：AI 笔记/读书整理" />
        <datalist id="ai-save-paths">
          {#each pathSuggestions as p (p)}
            <option value={p}></option>
          {/each}
        </datalist>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={rememberDefault} />
        <span class="muted">记住此库与位置为默认保存位置</span>
      </label>

      {#if err}
        <p class="error">{err}</p>
      {/if}
    </div>
    <div class="actions">
      <button type="button" onclick={onClose} disabled={saving}>取消</button>
      <button type="submit" class="primary" disabled={saving || !title.trim()}>
        {saving ? "保存中…" : "确定创建"}
      </button>
    </div>
    </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 960; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(440px, calc(100vw - 32px)); }
  h2 { margin: 0; font-size: 18px; color: var(--accent); }
  .dialog-content { display: flex; flex-direction: column; gap: 12px; }
  label { display: flex; flex-direction: column; gap: 5px; font-size: 13px; }
  label input, label select { width: 100%; box-sizing: border-box; font-size: 13px; }
  .check-row { flex-direction: row; align-items: center; gap: 6px; font-size: 12px; cursor: pointer; }
  .check-row input { width: auto; }
  .hint { margin: -6px 0 0; font-size: 12px; }
  .error { color: var(--hl); font-size: 13px; margin: 0; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
