<script>
  // One volume, one original line per row, with the line number (lb) in the
  // margin. `target` is a line to scroll to; hits of the query are highlighted.
  import { tick } from "svelte";

  let { vol, target = null, onvolume, onchapter } = $props();
  let scroller = $state();
  let flash = $state(null);

  $effect(() => {
    const t = target;
    const v = vol;
    if (!v) return;
    tick().then(() => {
      if (t) {
        const el = scroller?.querySelector(`[data-lb="${t}"]`);
        if (el) {
          el.scrollIntoView({ block: "center" });
          flash = t;
          return;
        }
      }
      if (scroller) scroller.scrollTop = 0;
    });
  });

  function chapterChange(e) {
    const c = vol.chapters[e.target.value];
    e.target.value = "";
    if (c) onchapter(c);
  }
</script>

{#if vol}
  <div class="reader">
    <header>
      <div class="title">
        <h1>{vol.title} <span class="juan">{vol.label}</span></h1>
        <div class="meta muted">{vol.book_id} · {vol.author} · {vol.path}</div>
      </div>
      <div class="nav">
        <select aria-label="卷" value={vol.path} onchange={(e) => onvolume(e.target.value)}>
          {#each vol.volumes as v}
            <option value={v.path} disabled={!v.exists}>{v.label}{v.exists ? "" : "（缺）"}</option>
          {/each}
        </select>
        {#if vol.chapters.length}
          <select aria-label="目次" onchange={chapterChange}>
            <option value="">目次…</option>
            {#each vol.chapters as c, i}
              <option value={i}>{"　".repeat(c.depth)}{c.title}</option>
            {/each}
          </select>
        {/if}
        {#if vol.hits}<span class="muted">本卷命中 {vol.hits} 处</span>{/if}
      </div>
    </header>
    <div class="lines" bind:this={scroller}>
      {#each vol.lines as line}
        <div class="line" class:flash={flash === line.lb} data-lb={line.lb}>
          <span class="lb">{line.lb}</span>
          <span class="txt">{#each line.parts as p}{#if p.hl}<span class="hl">{p.t}</span>{:else}{p.t}{/if}{/each}</span>
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .reader { display: flex; flex-direction: column; height: 100%; }
  header {
    display: flex; justify-content: space-between; align-items: flex-end; gap: 16px;
    padding: 12px 24px; border-bottom: 1px solid var(--line); background: var(--paper);
  }
  h1 { margin: 0; font-size: 22px; font-weight: 600; }
  .juan { font-size: 16px; color: var(--accent); margin-left: 6px; }
  .meta { font-size: 13px; margin-top: 4px; }
  .nav { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .lines { flex: 1; overflow: auto; padding: 16px 24px 48px; background: var(--paper); }
  .line { display: flex; gap: 16px; line-height: 2; font-size: 19px; border-radius: 4px; }
  .line.flash { background: var(--focus); animation: fade 2.5s ease-out 0.5s forwards; }
  @keyframes fade { to { background: transparent; } }
  .lb {
    flex: none; width: 64px; text-align: right; font-family: ui-monospace, Consolas, monospace;
    font-size: 12px; color: var(--muted); padding-top: 6px; user-select: none;
  }
  .txt { letter-spacing: 0.04em; }
</style>
