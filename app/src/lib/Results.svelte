<script>
  // Search results: facets on the left, hits with snippets on the right.
  // A snippet opens the volume at its line.
  // Results are grouped by library (design-spec 8.2): the chips show the
  // hit count of every library; facets and hits are those of one library.
  let { result, scope, rangeLabel = "", drill, onfacet, ondrill, onclearscope, onhit, onpage, onlib } = $props();
</script>

<div class="results">
  <aside class="facets">
    {#if scope || rangeLabel}
      <div class="scope">
        范围：<b>{[rangeLabel, scope].filter(Boolean).join(" › ")}</b>
        <button class="link" onclick={onclearscope}>清除</button>
      </div>
    {/if}
    {#each result.facets as f}
      {#if f.values.length}
        <section>
          <h3>{f.root === drill ? `${drill.split("/").pop()} ›` : f.label}</h3>
          <ul>
            {#each f.values.slice(0, 30) as v}
              <li>
                <button class="link" onclick={() => onfacet(f.root, v.name)}>{v.name}</button>
                <span class="muted">{v.count}</span>
                {#if f.root === "/bulei" && f.root !== drill}
                  <button class="link sub" onclick={() => ondrill(`${f.root}/${v.name}`)} title="展开子类">›</button>
                {/if}
              </li>
            {/each}
            {#if f.values.length > 30}<li class="muted">… 共 {f.values.length} 项</li>{/if}
          </ul>
        </section>
      {/if}
    {/each}
  </aside>

  <div class="hits">
    {#if result.libraries.length > 1}
      <div class="libs" role="tablist" aria-label="知识库">
        {#each result.libraries as l}
          <button role="tab" class:on={l.id === result.lib} aria-selected={l.id === result.lib} onclick={() => onlib(l.id)}>
            {l.title} <span class="n">{l.count}</span>
          </button>
        {/each}
      </div>
    {/if}
    <div class="summary muted">
      共 {result.total_volumes} 卷，{result.total_books} 部经 · {result.millis.toFixed(0)} ms
      {#if result.pages > 1}· 第 {result.page} / {result.pages} 页{/if}
    </div>
    {#each result.hits as h}
      <article>
        <h2>
          <button class="link" onclick={() => onhit(h, null)}>{h.title} {h.book_id ? h.subtitle : ""}</button>
          <span class="muted id">{[h.book_id, h.book_id ? "" : h.subtitle].filter(Boolean).join(" · ")} · {h.occurrences} 处</span>
        </h2>
        {#each h.snippets as s}
          <button class="snippet" onclick={() => onhit(h, s.lb)}>
            <span class="where muted">{s.lb} 第{s.pos}字</span>
            <span>…{#each s.parts as p}{#if p.hl}<span class="hl">{p.t}</span>{:else}{p.t}{/if}{/each}…</span>
          </button>
        {/each}
      </article>
    {:else}
      <p class="muted">没有找到。宽松匹配要求每个二字组都出现在同一卷；可以用空格分开关键词。</p>
    {/each}
    {#if result.pages > 1}
      <nav class="pager">
        <button disabled={result.page <= 1} onclick={() => onpage(result.page - 1)}>上一页</button>
        <span class="muted">{result.page} / {result.pages}</span>
        <button disabled={result.page >= result.pages} onclick={() => onpage(result.page + 1)}>下一页</button>
      </nav>
    {/if}
  </div>
</div>

<style>
  .results { display: flex; height: 100%; }
  .facets {
    width: 230px; flex: none; overflow: auto; padding: 12px 14px;
    border-right: 1px solid var(--line); background: var(--panel); font-size: 14px;
  }
  .scope { margin-bottom: 10px; padding: 6px 8px; background: var(--accent-soft); border-radius: 6px; }
  section { margin-bottom: 14px; }
  h3 { font-size: 13px; color: var(--muted); margin: 0 0 4px; font-weight: 600; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { display: flex; gap: 6px; align-items: baseline; line-height: 1.8; }
  li .link { text-align: left; color: var(--ink); }
  li .link:hover { color: var(--accent); }
  .sub { color: var(--accent) !important; }
  .hits { flex: 1; overflow: auto; padding: 12px 24px 40px; background: var(--paper); }
  .summary { font-size: 13px; margin-bottom: 8px; }
  .libs { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 10px; }
  .libs button { border-radius: 14px; font-size: 13px; padding: 2px 12px; }
  .libs button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .libs .n { opacity: 0.75; margin-left: 4px; }
  article { border-bottom: 1px solid var(--line); padding: 10px 0 12px; }
  h2 { font-size: 18px; margin: 0 0 6px; font-weight: 600; display: flex; gap: 10px; align-items: baseline; }
  .id { font-size: 13px; font-weight: normal; }
  .snippet {
    display: flex; gap: 12px; width: 100%; text-align: left; border: none; background: none;
    padding: 4px 6px; line-height: 1.8; font-size: 16px; border-radius: 4px;
  }
  .snippet:hover { background: var(--accent-soft); }
  .where { flex: none; width: 110px; font-size: 12px; font-family: ui-monospace, Consolas, monospace; padding-top: 4px; }
  .pager { display: flex; gap: 12px; align-items: center; justify-content: center; padding: 16px; }
</style>
