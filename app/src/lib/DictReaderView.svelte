<script>
  import { emit } from "@tauri-apps/api/event";
  import * as api from "./api.js";
  import ContextMenu from "./ContextMenu.svelte";
  import PinyinDialog from "./PinyinDialog.svelte";
  import { makePinyinItem } from "./pinyinMenu.svelte.js";
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte.js";
  import { ensureSimplifier, simplifyHtml, simplifyText } from "./simplify.svelte.js";
  import { openDictWindow, openDictEntryWindow } from "./dictWindow.js";
  import { sanitizeHtml } from "./sanitize.js";

  let {
    detail = null,
    loading = false,
    showPopout = false,
    onlookup = null,
  } = $props();

  let scriptMode = $state(settings.readerScript === "simp" ? "simp" : "orig");
  let scroller = $state();
  let menu = $state(null);
  let copied = $state(false);
  let pinyinDlg = $state("");

  $effect(() => {
    if (scriptMode === "simp") ensureSimplifier();
  });

  const shownHtml = $derived.by(() => {
    if (!detail?.html) return "";
    const safe = sanitizeHtml(detail.html);
    return scriptMode === "simp" ? simplifyHtml(safe) : safe;
  });

  const clip = (s, n = 14) => (s.length > n ? s.slice(0, n) + "…" : s);

  async function findInPage(text) {
    if (text && window.getSelection()?.toString().trim() !== text) {
      window.find?.(text, false, false, true);
    }
    await new Promise((r) => setTimeout(r, 60));
    try {
      await api.findInPage();
    } catch {
      if (text) window.find?.(text, false, false, true);
    }
  }

  function forwardAction(type, text, extra = {}) {
    emit("popout-action", {
      type,
      text,
      vol: { title: detail?.title || "词典" },
      ...extra,
    });
  }

  function onContextMenu(e) {
    const s = window.getSelection();
    const inside = s && !s.isCollapsed && scroller?.contains(s.anchorNode) && scroller?.contains(s.focusNode);
    const text = inside ? s.toString().trim() : "";
    const extEl = e.target.closest?.('a[href^="http://"], a[href^="https://"]');
    const extHref = extEl?.getAttribute("href") || "";
    e.preventDefault();
    const q = clip(text);
    const source = detail ? `${detail.title}（${detail.dict_name}）` : "词典";
    const items = [];
    if (extHref) {
      items.push({ label: "复制链接", action: () => navigator.clipboard.writeText(extHref).catch(() => {}) });
      if (text) items.push("separator");
    }
    if (text) {
      items.push(
        { label: "复制文字", action: () => navigator.clipboard.writeText(text).catch(() => {}) },
        "separator",
        { label: `全文检索：${q}`, action: () => forwardAction("search", text) },
        { label: `全文匹配：${q}`, action: () => forwardAction("phrase", text) },
        { label: `标题快搜：${q}`, action: () => forwardAction("quick", text) },
        { label: `页内查找：${q}`, action: () => findInPage(text) },
        "separator",
        makePinyinItem(text, () => (pinyinDlg = text)),
        {
          label: `查词典：${q}`,
          action: () => openDictWindow(text),
        },
      );
      if (settings.aiOn) {
        items.push(
          "separator",
          { label: "发给 AI 助手", action: () => forwardAction("ai-external", text, { autoSend: false, source }) },
          { label: "追加到 AI 会话", action: () => forwardAction("ai-external", text, { autoSend: false, append: true, source }) },
          {
            label: "AI 解释选中文字",
            action: () =>
              forwardAction("ai-external", text, {
                prompt: "请用通俗准确的现代汉语解释以上词典释义，并说明其核心义理与背景。",
                autoSend: true,
                source,
              }),
          },
        );
      }
    } else if (!extHref) {
      items.push(
        { label: "页内查找…", action: () => findInPage("") },
        { label: "复制全部释义", action: copyDetail, disabled: !detail },
      );
    }
    if (items.length) menu = { x: e.clientX, y: e.clientY, items };
  }

  function onDetailClick(e) {
    const seealso = e.target.closest?.("a.dict-seealso");
    if (seealso) {
      e.preventDefault();
      const term = seealso.textContent?.trim();
      if (!term) return;
      if (onlookup) onlookup(term);
      else openDictWindow(term);
      return;
    }
    const ext = e.target.closest?.('a[href^="http://"], a[href^="https://"]');
    if (ext) {
      e.preventDefault();
      const href = ext.getAttribute("href");
      if (href) api.openExternalWindow(href, settings.aiOn).catch(() => {});
    }
  }

  async function copyDetail() {
    if (!detail?.text) return;
    const txt = scriptMode === "simp" ? simplifyText(detail.text) : detail.text;
    try {
      await navigator.clipboard.writeText(txt);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {}
  }

  function popoutEntry() {
    if (!detail) return;
    openDictEntryWindow({
      dictId: detail.dict_id,
      entryId: detail.entry_id,
      title: detail.title,
    });
  }
</script>

<div class="dict-reader">
  {#if detail}
    <div class="reader-tools">
      <span class="muted dict-source-label" title={detail.dict_name}>{detail.dict_name}</span>
      <div class="tool-right">
        <div class="seg" role="group" aria-label="繁简切换">
          <button type="button" class:on={scriptMode === "orig"} onclick={() => (scriptMode = "orig")}>原文</button>
          <button type="button" class:on={scriptMode === "simp"} onclick={() => (scriptMode = "simp")}>简体</button>
        </div>
        <button type="button" onclick={copyDetail}>{copied ? "已复制" : "复制释义"}</button>
        {#if showPopout}
          <button type="button" class="icon-btn" onclick={popoutEntry} title="在新窗口打开此词条" aria-label="在新窗口打开此词条">
            <Icon name="open-window" size={14} />
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="detail-scroll" bind:this={scroller} onclick={onDetailClick} oncontextmenu={onContextMenu}>
    {#if loading && !detail}
      <div class="empty muted">正在加载词条内容…</div>
    {:else if !detail}
      <div class="empty muted">选择左侧词条查看释义</div>
    {:else}
      <div class="detail-item">
        {@html shownHtml}
      </div>
    {/if}
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

{#if pinyinDlg}
  <PinyinDialog text={pinyinDlg} onclose={() => (pinyinDlg = "")} />
{/if}

<style>
  .dict-reader {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .reader-tools {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 14px;
    border-bottom: 1px solid var(--line);
    background: var(--paper);
    font-size: 12px;
    flex: none;
  }
  .dict-source-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool-right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }
  .seg { display: flex; }
  .seg button { border-radius: 0; margin-left: -1px; padding: 2px 8px; font-size: 12px; }
  .seg button:first-child { border-radius: 5px 0 0 5px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 5px 5px 0; }
  .seg button.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--paper);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px 6px;
  }
  .detail-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 18px 26px;
    font-family: var(--reader-font, var(--font-serif));
    font-size: var(--reader-size, 18px);
    line-height: var(--reader-line, 1.9);
    font-weight: var(--reader-weight, 400);
    -webkit-text-stroke-width: var(--reader-stroke, 0);
    user-select: text;
  }
  .detail-scroll :global(b), .detail-scroll :global(strong) {
    font-weight: var(--reader-strong-weight, 700);
    -webkit-text-stroke-width: var(--reader-strong-stroke, 0);
  }
  .detail-scroll :global(.dict-entry-title) {
    text-align: center;
    margin: 0 0 4px;
    font-size: 1.35em;
    color: var(--accent);
  }
  .detail-scroll :global(.dict-entry-title a),
  .detail-scroll :global(a.dict-seealso) {
    color: var(--accent);
    text-decoration: none;
  }
  .detail-scroll :global(a.dict-seealso:hover) {
    text-decoration: underline;
  }
  .detail-scroll :global(.dict-entry-body a.dict-seealso::before) {
    content: "";
    display: inline-block;
    width: 0.95em;
    height: 0.95em;
    margin-right: 0.2em;
    vertical-align: -0.12em;
    background: currentColor;
    mask: center / contain no-repeat;
  }
  .detail-scroll :global(.dict-entry-source) {
    text-align: center;
    font-size: 0.85em;
    color: var(--muted);
    margin-bottom: 14px;
  }
  .detail-scroll :global(.dict-entry-body img) {
    max-width: 100%;
    height: auto;
    vertical-align: middle;
  }
  .empty {
    padding: 36px 16px;
    text-align: center;
    font-size: 14px;
  }
</style>
