<script>
  // One volume in two views:
  //  * 段落 (default): the structured HTML of the volume (design-spec A.6):
  //    divisions, headings, verses, lists, tables, 科判 diagrams, images,
  //    inline notes, collation notes as numbered bubbles, variant readings
  //    on hover; line numbers can be shown in the text;
  //  * 原书行: one original line per row, line number in the margin.
  // `target` is a line (lb) to scroll to; hits of the query are highlighted
  // in both views.
  import { tick } from "svelte";

  let { vol, target = null, onvolume, onchapter } = $props();
  let scroller = $state();

  const load = (k, d) => {
    try {
      return localStorage.getItem(k) ?? d;
    } catch {
      return d;
    }
  };
  const save = (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  };
  let mode = $state(load("reader.mode", "para")); // para | lines
  let showLb = $state(load("reader.showLb", "0") === "1");
  $effect(() => save("reader.mode", mode));
  $effect(() => save("reader.showLb", showLb ? "1" : "0"));

  $effect(() => {
    const t = target;
    const v = vol;
    const m = mode;
    if (!v) return;
    tick().then(() => {
      if (!scroller) return;
      scroller.querySelectorAll(".flash").forEach((e) => e.classList.remove("flash"));
      if (t) {
        const el = scroller.querySelector(`[data-lb="${t}"]`);
        if (el) {
          el.scrollIntoView({ block: "center" });
          el.classList.add("flash");
          return;
        }
      }
      scroller.scrollTop = 0;
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
        <div class="seg" role="group" aria-label="版式">
          <button class:on={mode === "para"} onclick={() => (mode = "para")}>段落</button>
          <button class:on={mode === "lines"} onclick={() => (mode = "lines")}>原书行</button>
        </div>
        {#if mode === "para"}
          <label class="check"><input type="checkbox" bind:checked={showLb} /> 行号</label>
        {/if}
        {#if vol.hits}<span class="muted">本卷命中 {vol.hits} 处</span>{/if}
      </div>
    </header>
    <div class="scroll" bind:this={scroller}>
      {#if mode === "para"}
        <article class="doc" class:show-lb={showLb}>{@html vol.html}</article>
      {:else}
        <div class="lines">
          {#each vol.lines as line}
            <div class="line" data-lb={line.lb}>
              <span class="lbn">{line.lb}</span>
              <span class="txt">{#each line.parts as p}{#if p.hl}<mark>{p.t}</mark>{:else}{p.t}{/if}{/each}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .reader { display: flex; flex-direction: column; height: 100%; }
  header {
    display: flex; justify-content: space-between; align-items: flex-end; gap: 16px; flex-wrap: wrap;
    padding: 12px 24px; border-bottom: 1px solid var(--line); background: var(--paper);
  }
  h1 { margin: 0; font-size: 22px; font-weight: 600; }
  .juan { font-size: 16px; color: var(--accent); margin-left: 6px; }
  .meta { font-size: 13px; margin-top: 4px; }
  .nav { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 14px; }
  .seg { display: inline-flex; }
  .seg button { border-radius: 0; padding: 4px 10px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; }
  .seg button:last-child { border-radius: 0 6px 6px 0; border-left: none; }
  .seg button.on { background: var(--accent); color: var(--paper); border-color: var(--accent); }
  .check { display: flex; gap: 4px; align-items: center; }
  .scroll { flex: 1; overflow: auto; background: var(--paper); }

  /* original lines */
  .lines { padding: 16px 24px 48px; }
  .line { display: flex; gap: 16px; line-height: 2; font-size: 19px; border-radius: 4px; }
  .line:global(.flash) { background: var(--focus); animation: fade 2.5s ease-out 0.5s forwards; }
  .lbn {
    flex: none; width: 64px; text-align: right; font-family: ui-monospace, Consolas, monospace;
    font-size: 12px; color: var(--muted); padding-top: 6px; user-select: none;
  }
  .txt { letter-spacing: 0.04em; }
  @keyframes fade { to { background: transparent; } }

  /* structured view: the HTML comes from core/src/render.rs */
  .doc { max-width: 46em; margin: 0 auto; padding: 24px 32px 64px; font-size: 19px; line-height: 2; letter-spacing: 0.04em; }
  .doc :global(mark) { color: var(--hl); background: var(--hl-bg); border-radius: 2px; }
  .doc :global(.div) { margin: 0.2em 0; }
  .doc :global(.head) { font-weight: 700; font-size: 1.08em; margin: 1em 0 0.4em; color: var(--accent); }
  .doc :global(.juan) { font-weight: 700; margin: 0.6em 0; }
  .doc :global(.juan.f-close) { margin-top: 2em; color: var(--muted); }
  .doc :global(.byline) { margin: 0.2em 0 0.2em 6em; color: var(--muted); }
  .doc :global(.docnumber) { margin: 0; font-size: 0.75em; color: var(--muted); }
  .doc :global(p.p) { margin: 0.35em 0; }
  .doc :global(p.r-text-right) { text-align: right; }
  .doc :global(p.t-pre) { white-space: pre; line-height: 1.5; overflow-x: auto; font-size: 0.9em; }
  .doc :global(p.t-dharani) { color: var(--ink); }
  .doc :global(.lg) { margin: 0.5em 0 0.5em 2em; }
  .doc :global(.l) { margin: 0; }
  .doc :global(.caesura) { display: inline-block; width: 1.5em; }
  .doc :global(.inline-note) { font-size: 0.8em; color: var(--muted); }
  .doc :global(.list) { list-style: none; margin: 0.3em 0; padding-left: 1.5em; }
  .doc :global(.table) { border-collapse: collapse; margin: 0.5em 0; font-size: 0.95em; }
  .doc :global(.table td) { border: 1px solid var(--line); padding: 2px 8px; vertical-align: top; }
  .doc :global(.entry) { margin: 0.4em 0; }
  .doc :global(.entry .form) { font-weight: 700; margin-right: 0.5em; }
  .doc :global(.entry .def), .doc :global(.entry .def p) { display: inline; }
  .doc :global(.sp) { margin: 0.3em 0 0.3em 1em; }
  .doc :global(.hi.r-bold) { font-weight: 700; }
  .doc :global(.hi.r-border) { border: 1px solid var(--muted); padding: 0 2px; }
  .doc :global(.hi.r-circle) { border: 1px solid var(--muted); border-radius: 50%; padding: 0 2px; }
  .doc :global(.g) { color: #8a3c6a; font-size: 0.85em; }
  .doc :global(.app[title]:not([title=""])) { border-bottom: 1px dotted var(--accent-soft); }
  .doc :global(.tt[title]:not([title=""])) { border-bottom: 1px dotted var(--muted); }
  .doc :global(.unclear) { color: var(--muted); }
  .doc :global(img.figure) { max-width: 100%; display: block; margin: 0.5em 0; background: white; }
  .doc :global(.note) { position: relative; cursor: pointer; }
  .doc :global(.note sup) { color: var(--accent); font-size: 0.6em; padding: 0 1px; }
  .doc :global(.note .note-body) {
    display: none; position: absolute; left: 0; top: 1.6em; z-index: 5; width: max-content; max-width: 24em;
    background: var(--panel); border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px;
    font-size: 14px; line-height: 1.6; letter-spacing: 0; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    white-space: normal;
  }
  .doc :global(.note:hover .note-body), .doc :global(.note:focus .note-body) { display: block; }
  /* the jump marker must not open a line of its own between two paragraphs */
  .doc :global(.lb.flash) { position: relative; }
  .doc :global(.lb.flash)::before {
    content: "▼"; color: var(--hl); font-size: 0.55em; position: absolute; left: -0.5em; top: -1.3em;
  }
  .doc :global(.lb.flash) { scroll-margin: 40vh; }
  .doc.show-lb :global(.lb)::before {
    content: attr(data-lb); font-family: ui-monospace, Consolas, monospace; font-size: 10px; color: var(--muted);
    vertical-align: super; margin: 0 2px; letter-spacing: 0;
  }
  .doc.show-lb :global(.lb.flash)::before { color: var(--hl); font-weight: 700; }
</style>
