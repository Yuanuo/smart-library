<script>
  // Tags (and a note) for a book / article, or for a collected passage.
  // Tags are separated by commas; the tags already in use are offered.
  import { onMount } from "svelte";
  import * as api from "./api.js";

  let { heading, subtitle = "", quote = "", withNote = false, tags = "", note = "", okLabel = "保存", onok, onclose } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(tags);
  // svelte-ignore state_referenced_locally
  let noteText = $state(note);
  let known = $state([]);
  let input = $state();
  let busy = $state(false);
  let error = $state("");

  const current = $derived(
    text
      .split(/[,，、;；]/)
      .map((t) => t.trim())
      .filter(Boolean),
  );

  onMount(async () => {
    input?.focus();
    try {
      known = (await api.markTags()).slice(0, 40);
    } catch {}
  });

  function toggle(tag) {
    const has = current.includes(tag);
    const next = has ? current.filter((t) => t !== tag) : [...current, tag];
    text = next.join(", ");
  }

  async function submit(e) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = "";
    try {
      await onok(text, noteText);
      onclose();
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <form class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={heading} onsubmit={submit}
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>{heading}</h2></header>
    <div class="dialog-content">
    {#if subtitle}<p class="muted sub">{subtitle}</p>{/if}
    {#if quote}<blockquote>{quote}</blockquote>{/if}
    <label>标签（用逗号分隔）
      <input type="text" bind:value={text} bind:this={input} placeholder="例如：反馈, 待读" />
    </label>
    {#if known.length}
      <div class="chips" aria-label="已有的标签">
        {#each known as k}
          <button type="button" class="chip" class:on={current.includes(k.tag)} onclick={() => toggle(k.tag)}>{k.tag}</button>
        {/each}
      </div>
    {/if}
    {#if withNote}
      <label>备注（可选）<textarea rows="3" bind:value={noteText}></textarea></label>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button class="primary" disabled={busy}>{okLabel}</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(500px, calc(100vw - 32px)); }
  h2 { margin: 0 0 6px; font-size: 18px; color: var(--accent); }
  .sub { margin: 0 0 8px; font-size: 13px; }
  blockquote {
    margin: 6px 0 10px; padding: 6px 10px; border-left: 3px solid var(--accent); background: var(--bg);
    max-height: 7em; overflow: auto; white-space: pre-wrap;
  }
  label { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; }
  textarea { font: inherit; border: 1px solid var(--line); background: var(--paper); border-radius: 6px; padding: 6px 8px; color: inherit; resize: vertical; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .chip { padding: 1px 10px; border-radius: 12px; font-size: 13px; }
  .chip.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>
