<script>
  // A list of values as chips with a box to add more: Enter, comma or leaving
  // the box adds the typed text; Backspace on an empty box takes the last one
  // back. `suggestions` are offered while typing.
  let { values = $bindable([]), suggestions = [], presets = [], placeholder = "", onchange = null } = $props();
  const id = `chips-${Math.random().toString(36).slice(2, 8)}`;
  let text = $state("");

  function add() {
    const t = text.trim();
    text = "";
    if (!t || values.includes(t)) return;
    values = [...values, t];
    onchange?.();
  }
  function pick(v) {
    if (values.includes(v)) return;
    values = [...values, v];
    onchange?.();
  }
  function remove(i) {
    values = values.filter((_, j) => j !== i);
    onchange?.();
  }
  function key(e) {
    if (e.key === "Enter" || e.key === "," || e.key === "，" || e.key === "、") {
      e.preventDefault();
      add();
    } else if (e.key === "Backspace" && !text && values.length) {
      remove(values.length - 1);
    }
  }
</script>

<div class="wrap">
<div class="chips">
  {#each values as v, i (v)}
    <span class="chip">{v}<button type="button" class="link" onclick={() => remove(i)} aria-label="去掉 {v}">×</button></span>
  {/each}
  <input type="text" list={id} bind:value={text} onkeydown={key} onblur={add} {placeholder} />
  <datalist {id}>
    {#each suggestions.filter((s) => !values.includes(s)).slice(0, 300) as s (s)}<option value={s}></option>{/each}
  </datalist>
</div>
{#if presets.length}
  <div class="presets" aria-label="常用值，点一下加入">
    {#each presets.filter((p) => !values.includes(p)) as p (p)}
      <button type="button" onclick={() => pick(p)}>{p}</button>
    {/each}
  </div>
{/if}
</div>

<style>
  .wrap { flex: 1; min-width: 0; }
  .presets { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 5px; }
  .presets button { font-size: 12px; padding: 0 8px; border-radius: 10px; line-height: 20px; color: var(--accent); background: none; }
  .presets button:hover { background: var(--accent-soft); }
  .chips {
    display: flex; flex-wrap: wrap; gap: 4px; align-items: center; flex: 1; min-width: 0;
    padding: 3px 6px; border: 1px solid var(--line); border-radius: 6px; background: var(--paper);
  }
  .chip { background: var(--accent-soft); border-radius: 12px; padding: 0 4px 0 10px; font-size: 14px; }
  .chip .link { padding: 0 4px; color: var(--muted); }
  input { flex: 1; min-width: 90px; border: none; outline: none; background: none; font-size: 14px; padding: 3px 2px; }
</style>
