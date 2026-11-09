<script>
  // Properties of a node: the values of the search filters (部类, 时域, 人物,
  // 藏经, 典类) and its tags. What the data gives is shown and can be
  // replaced; a replaced value goes back with "恢复默认". Nothing is written
  // into the source data. For a folder or a note the properties also apply to
  // everything below it. More properties can join this dialog later.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import ChipInput from "./ChipInput.svelte";

  let { lib, libTitle = "", bookId = null, path = "", title = "", onsaved, onclose } = $props();

  const DIMS = [
    ["bulei", "部类", "用 / 分层，如 华严部类/华严部"],
    ["period", "时域", "朝代或时期，如 唐"],
    ["person", "人物", "作者、译者，可多个"],
    ["canon", "藏经", "如 大正新脩大藏經"],
    ["kind", "典类", "用 / 分层，如 论/经释"],
  ];

  // common values to pick with a click (names only: what is chosen is what the index gets)
  const PRESETS = {
    period: "新罗 东汉 曹魏 吴 西晋 东晋 符秦 前秦 姚秦 后秦 元魏 后魏 北凉 刘宋 南北朝 萧齐 梁 东魏 胡 陈 隋 唐 高丽 后晋 宋 金 元 明 清 民国 现代 日本 古印度 其他".split(" "),
    kind: ["经", "律", "论", "论/经释", "论/律释", "论/论释", "仪轨、念诵法", "传记", "史", "记录、纪事", "山志、寺志", "语录", "序", "目录", "文献", "音译", "其他"],
  };

  let rows = $state(Object.fromEntries(DIMS.map(([d]) => [d, { values: [], own: false }])));
  let derived = $state({});
  let suggestions = $state({});
  let tags = $state([]);
  let tags0 = "";
  let aliases = $state([]);
  let aliases0 = "";
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");

  const norm = (list) => [...new Set(list.map((v) => v.trim()).filter(Boolean))].sort().join("\u0001");

  onMount(async () => {
    try {
      const r = await api.propsGet(lib, bookId, path);
      derived = r.derived;
      for (const [d] of DIMS) {
        const own = r.own[d];
        rows[d] = own ? { values: [...own], own: true } : { values: [...(r.derived[d] ?? [])], own: false };
      }
      tags = r.tags;
      tags0 = norm(r.tags);
      aliases = [...(r.aliases ?? [])];
      aliases0 = (r.aliases ?? []).join(""); // the order is kept
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
    // values already in use in this library (when it has an index)
    try {
      const s = await api.search({ query: "", phrase: false, lib, limit: 1 });
      suggestions = Object.fromEntries(s.facets.map((f) => [f.root.slice(1), f.values.map((v) => v.name)]));
    } catch {}
  });

  function touched(d) {
    rows[d].own = true;
  }
  function restore(d) {
    rows[d] = { values: [...(derived[d] ?? [])], own: false };
  }

  async function save() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const dims = {};
      for (const [d] of DIMS) {
        const r = rows[d];
        // only what differs from the data is kept as the user's own
        if (r.own && norm(r.values) !== norm(derived[d] ?? [])) dims[d] = r.values;
      }
      if (norm(tags) !== tags0) await api.nodeTagsSet(lib, bookId, path, title, tags.join(","));
      const res = await api.propsSet(lib, bookId, path, dims, aliases.join("") !== aliases0 ? aliases : null);
      onsaved?.(res);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <form class="dialog" role="dialog" aria-modal="true" aria-label="属性" onsubmit={(e) => (e.preventDefault(), save())}
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>属性：{title}</h2>
    <p class="muted sub">{libTitle}</p>

    {#if loading}
      <p class="muted">载入中…</p>
    {:else}
      <fieldset>
        <legend>搜索过滤</legend>
        {#each DIMS as [d, label, hint]}
          <div class="row">
            <span class="lab">{label}</span>
            <div class="field">
              <ChipInput bind:values={rows[d].values} suggestions={suggestions[d] ?? []} presets={PRESETS[d] ?? []} placeholder={hint} onchange={() => touched(d)} />
              <div class="state muted">
                {#if rows[d].own}
                  已修改{#if (derived[d] ?? []).length}（默认：{derived[d].join("、")}）{:else}（默认：无）{/if}
                  <button type="button" class="link" onclick={() => restore(d)}>恢复默认</button>
                {:else if rows[d].values.length}
                  来自数据
                {/if}
              </div>
            </div>
          </div>
        {/each}
        <p class="muted hint">
          这些值用在全文搜索的过滤里（改动后会更新索引），不会改动原文件。对文件夹或笔记设置时，它下面的内容都适用；下层另有设置时以下层为准。
        </p>
      </fieldset>

      <fieldset>
        <legend>别名</legend>
        <div class="row">
          <div class="field"><ChipInput bind:values={aliases} placeholder="这个节点的其他名字，可多个，如 法华经、莲华经" /></div>
        </div>
        <p class="muted hint">标题快搜里输入别名也能找到这个节点（不会继承给下面的内容，也不是搜索过滤）。</p>
      </fieldset>

      <fieldset>
        <legend>标签</legend>
        <div class="row">
          <div class="field"><ChipInput bind:values={tags} placeholder="如 待读、精读" /></div>
        </div>
      </fieldset>
    {/if}

    {#if error}<p class="err">{error}</p>{/if}
    <div class="actions">
      <button type="button" onclick={onclose}>取消</button>
      <button class="primary" disabled={busy || loading}>{busy ? "保存中…" : "保存"}</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(620px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto;
    background: var(--paper); border-radius: 10px; padding: 18px 24px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0; font-size: 20px; color: var(--accent); overflow-wrap: anywhere; }
  .sub { margin: 2px 0 8px; font-size: 13px; }
  fieldset { border: 1px solid var(--line); border-radius: 8px; margin: 10px 0; padding: 4px 14px 10px; }
  legend { padding: 0 6px; font-weight: 600; }
  .row { display: flex; gap: 12px; align-items: flex-start; margin: 8px 0; }
  .lab { width: 44px; flex: none; padding-top: 6px; }
  .field { flex: 1; min-width: 0; }
  .state { font-size: 12px; min-height: 16px; padding: 2px 2px 0; }
  .hint { font-size: 13px; margin: 4px 0 0; }
  .err { color: var(--hl); white-space: pre-wrap; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
