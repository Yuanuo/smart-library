<script>
  // A small dialog asking for one line of text (a note title, a library name).
  import { onMount } from "svelte";

  // `hint`: a rule shown under the box; `validate(text)`: resolves to the reason
  // the text is not acceptable (or "" when it is), checked as the user types.
  let { title, label = "名称", value = "", okLabel = "确定", hint = "", validate = null, allowEmpty = false, onok, onclose } = $props();
  let text = $state(value);
  let input = $state();
  let busy = $state(false);
  let error = $state("");
  let bad = $state("");

  $effect(() => {
    const t = text.trim();
    if (!validate || !t) {
      bad = "";
      return;
    }
    let stale = false;
    Promise.resolve(validate(t)).then((e) => {
      if (!stale) bad = e || "";
    });
    return () => (stale = true);
  });

  onMount(() => {
    input?.focus();
    input?.select();
  });

  async function submit(e) {
    e.preventDefault();
    if ((!allowEmpty && !text.trim()) || busy || bad) return;
    busy = true;
    error = "";
    try {
      await onok(text.trim());
      onclose();
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <form class="dialog" role="dialog" aria-modal="true" aria-label={title} onsubmit={submit}
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>{title}</h2>
    <label>{label}<input type="text" bind:value={text} bind:this={input} /></label>
    {#if bad}<p class="error">{bad}</p>{:else if error}<p class="error">{error}</p>{/if}
    {#if hint}<p class="hint muted">{hint}</p>{/if}
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button class="primary" disabled={(!allowEmpty && !text.trim()) || busy || !!bad}>{okLabel}</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(420px, calc(100vw - 32px)); background: var(--paper); border-radius: 10px;
    padding: 18px 22px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 10px; font-size: 18px; color: var(--accent); }
  label { display: flex; flex-direction: column; gap: 4px; }
  .error { color: var(--hl); font-size: 13px; }
  .hint { font-size: 12px; margin: 6px 0 0; white-space: pre-line; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>
