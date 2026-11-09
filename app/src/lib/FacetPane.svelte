<script>
  // One facet dimension as a list of check boxes (several can be ticked: one
  // of the ticked values of a dimension, and every ticked dimension, must
  // match). Long lists are cut into a window of rows, can be narrowed by
  // typing, sorted by count or by pinyin, and (by pinyin) jumped to by letter.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  let { facet, selected = [], ontoggle, onclear } = $props();

  // periods are ordered by the first year of the table of periods (names and other names)
  const isPeriod = $derived(facet.root === "/period");
  let years = $state(new Map());
  onMount(() => {
    if (!isPeriod) return;
    api
      .dynastyTable()
      .then((rows) => {
        const m = new Map();
        for (const r of rows) {
          m.set(r.name, r.start);
          for (const a of r.aliases) if (!m.has(a)) m.set(a, r.start);
        }
        years = m;
      })
      .catch(() => {});
  });
  const yearOf = (n) => years.get(n) ?? 1e9;

  const ROW = 28;
  let collator;
  try {
    collator = new Intl.Collator("zh-u-co-pinyin");
  } catch {
    collator = new Intl.Collator("zh");
  }
  const byName = (a, b) => collator.compare(a, b);

  // two levels (`部類` and `部類/子類`) are listed together, children indented
  const twoLevel = $derived(facet.root === "/bulei" || facet.root === "/kind");

  // sorted by count or pinyin; the choice is remembered per dimension
  let sort = $state(initialSort());
  function initialSort() {
    try {
      const v = localStorage.getItem(`facet.sort${facet.root}`);
      if (v === "count" || v === "pinyin" || (v === "time" && facet.root === "/period")) return v;
    } catch {}
    return facet.root === "/person" ? "pinyin" : facet.root === "/period" ? "time" : "count";
  }
  function setSort(v) {
    sort = v;
    try {
      localStorage.setItem(`facet.sort${facet.root}`, v);
    } catch {}
  }

  let text = $state("");
  const sel = $derived(new Set(selected));

  // the values, plus ticked ones that have no hit now (so they can be unticked)
  const all = $derived.by(() => {
    const m = new Map(facet.values.map((v) => [v.name, v.count]));
    for (const n of selected) if (!m.has(n)) m.set(n, 0);
    return [...m].map(([name, count]) => ({ name, count }));
  });

  const flatCmp = $derived(
    sort === "count"
      ? (a, b) => b.count - a.count || byName(a.name, b.name)
      : sort === "time"
        ? (a, b) => yearOf(a.name) - yearOf(b.name) || byName(a.name, b.name)
        : (a, b) => byName(a.name, b.name),
  );

  const rows = $derived.by(() => {
    const q = text.trim().toLowerCase();
    if (q) {
      return all
        .filter((v) => v.name.toLowerCase().includes(q))
        .sort(flatCmp)
        .map((v) => ({ ...v, label: v.name, child: false }));
    }
    if (!twoLevel) return [...all].sort(flatCmp).map((v) => ({ ...v, label: v.name, child: false }));
    const groups = new Map(); // top -> { top, kids }
    for (const v of all) {
      const i = v.name.indexOf("/");
      const top = i < 0 ? v.name : v.name.slice(0, i);
      if (!groups.has(top)) groups.set(top, { top: null, kids: [] });
      const g = groups.get(top);
      if (i < 0) g.top = v;
      else g.kids.push({ ...v, label: v.name.slice(i + 1), child: true });
    }
    const list = [...groups.entries()].map(([name, g]) => ({ name, ...g }));
    list.sort(sort === "count" ? (a, b) => (b.top?.count ?? 0) - (a.top?.count ?? 0) || byName(a.name, b.name) : (a, b) => byName(a.name, b.name));
    return list.flatMap((g) => [...(g.top ? [{ ...g.top, label: g.top.name, child: false }] : []), ...g.kids.sort(flatCmp)]);
  });

  // ---- jump by initial letter (pinyin order only)
  const BOUNDS = "阿八嚓搭蛾发噶哈击喀垃妈拿哦啪期然撒塌挖昔压匝".split("");
  const LETTERS = "ABCDEFGHJKLMNOPQRSTWXYZ".split("");
  const cache = new Map();
  function letterOf(name) {
    const c = name[0] ?? "";
    if (cache.has(c)) return cache.get(c);
    let r = "#";
    if (/[A-Za-z]/.test(c)) r = c.toUpperCase();
    else if (/[㐀-鿿豈-﫿]/.test(c)) {
      r = "A";
      for (let i = BOUNDS.length - 1; i >= 0; i--) {
        if (collator.compare(c, BOUNDS[i]) >= 0) {
          r = LETTERS[i];
          break;
        }
      }
    }
    cache.set(c, r);
    return r;
  }
  const letters = $derived.by(() => {
    if (sort !== "pinyin" || text.trim() || rows.length < 50) return [];
    const first = new Map();
    rows.forEach((r, i) => {
      if (r.child) return;
      const l = letterOf(r.name);
      if (!first.has(l)) first.set(l, i);
    });
    return [...first];
  });

  // ---- a window of rows
  let box = $state();
  let height = $state(300);
  let scrollTop = $state(0);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 8));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + 8));
  const shown = $derived(rows.slice(start, end));
  function jump(index) {
    if (box) box.scrollTop = index * ROW;
  }
  $effect(() => {
    text; // a new filter text starts at the top
    sort;
    if (box) box.scrollTop = 0;
  });
</script>

<div class="pane">
  <div class="head">
    <input type="search" placeholder="在 {facet.label} 中查找" bind:value={text} aria-label="筛选 {facet.label} 项" />
    <span class="seg" role="radiogroup" aria-label="排序">
      <button type="button" class:on={sort === "count"} onclick={() => setSort("count")} title="按命中数从多到少">数量</button>
      <button type="button" class:on={sort === "pinyin"} onclick={() => setSort("pinyin")} title="按拼音 A–Z">拼音</button>
      {#if isPeriod}<button type="button" class:on={sort === "time"} onclick={() => setSort("time")} title="按时域表里的起始年份，先古后今">年代</button>{/if}
    </span>
  </div>
  <div class="sub muted">
    共 {all.length} 项{#if selected.length}　已选 {selected.length}　<button type="button" class="link" onclick={onclear}>清除</button>{/if}
  </div>
  <div class="body">
    <div class="list" bind:this={box} bind:clientHeight={height} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
      <div class="space" style="height: {rows.length * ROW}px">
        {#each shown as r, i (r.name)}
          <label class="row" class:child={r.child} class:top={twoLevel && !r.child && !text.trim()} class:zero={r.count === 0} style="top: {(start + i) * ROW}px" title={r.name}>
            <input type="checkbox" checked={sel.has(r.name)} onchange={() => ontoggle(r.name)} />
            <span class="name">{r.label}</span>
            <span class="n">{r.count}</span>
          </label>
        {/each}
      </div>
      {#if !rows.length}
        <p class="muted none">{text.trim() ? "没有匹配的项。" : "没有可选项。"}</p>
      {/if}
    </div>
    {#if letters.length}
      <div class="letters" aria-label="按拼音首字母跳转">
        {#each letters as [l, i]}
          <button type="button" onclick={() => jump(i)}>{l}</button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .pane { display: flex; flex-direction: column; min-height: 0; height: 100%; }
  .head { display: flex; gap: 6px; align-items: center; padding: 6px 8px 2px; }
  .head input { flex: 1; min-width: 0; font-size: 13px; }
  .seg { display: inline-flex; flex: none; }
  .seg button { font-size: 12px; padding: 2px 8px; border-radius: 0; }
  .seg button:first-child { border-radius: 6px 0 0 6px; }
  .seg button:last-child { border-radius: 0 6px 6px 0; margin-left: -1px; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .sub { font-size: 12px; padding: 2px 10px 4px; }
  .body { flex: 1; min-height: 0; display: flex; }
  .list { flex: 1; min-width: 0; overflow-y: auto; overflow-x: hidden; position: relative; }
  .space { position: relative; }
  .row {
    position: absolute; left: 0; right: 0; height: 28px; display: flex; align-items: center; gap: 6px;
    padding: 0 8px; border-bottom: 1px dashed var(--line); font-size: 14px; cursor: pointer; box-sizing: border-box;
  }
  .row:hover { background: var(--accent-soft); }
  .row.child { padding-left: 28px; font-size: 13px; }
  .row.top { font-weight: 600; }
  .row.zero { opacity: 0.55; }
  .row input { flex: none; margin: 0; }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .n { flex: none; color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .none { padding: 8px 12px; }
  .letters {
    flex: none; width: 20px; overflow-y: auto; display: flex; flex-direction: column; align-items: center;
    padding: 2px 0; border-left: 1px solid var(--line);
  }
  .letters button { border: none; background: none; padding: 0; font-size: 11px; line-height: 17px; width: 18px; color: var(--accent); }
  .letters button:hover { background: var(--accent-soft); }
</style>
