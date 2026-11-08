<script>
  // A mind map note: the data (Mind-Elixir JSON) is the note's content. It is
  // edited in place and saved a moment after every change (and with Ctrl+S).
  // A node's link to a place in a document (`smartlib://`) leads there.
  import { onMount, onDestroy, untrack } from "svelte";
  import * as api from "./api.js";
  import { parseSource } from "./clip.js";

  let { vol, lib, path, onjump = null } = $props();

  let el = $state();
  let mind = null;
  let timer = null;
  let dirty = false;
  let gone = false;
  let status = $state("");
  let error = $state("");
  const start = untrack(() => vol.raw);

  const dataText = () => JSON.stringify(mind.getData(), (k, v) => (k === "parent" ? undefined : v));

  function changed() {
    dirty = true;
    status = "未保存";
    clearTimeout(timer);
    timer = setTimeout(save, 800);
  }

  async function save() {
    if (!mind || !dirty) return;
    dirty = false;
    try {
      await api.saveNote(lib, path, dataText());
      if (!gone) status = "已保存";
    } catch (e) {
      dirty = true;
      error = String(e);
    }
  }

  const isDark = () => {
    const t = document.documentElement.getAttribute("data-theme");
    return t === "dark" || (!t && window.matchMedia("(prefers-color-scheme: dark)").matches);
  };

  onMount(async () => {
    try {
      const [{ default: MindElixir }, { zh_CN }] = await Promise.all([import("mind-elixir"), import("mind-elixir/i18n")]);
      await import("mind-elixir/style.css");
      if (gone) return;
      let data = null;
      try {
        data = JSON.parse(start);
      } catch {}
      if (!data?.nodeData) data = MindElixir.new(vol.title);
      mind = new MindElixir({
        el,
        direction: MindElixir.SIDE,
        toolBar: true,
        keypress: true,
        allowUndo: true,
        contextMenu: { locale: zh_CN },
        theme: isDark() ? MindElixir.DARK_THEME : MindElixir.THEME,
      });
      await mind.init(data);
      mind.bus.addListener("operation", changed);
    } catch (e) {
      error = String(e);
    }
  });

  onDestroy(() => {
    gone = true;
    clearTimeout(timer);
    if (mind && dirty) api.saveNote(lib, path, dataText()).catch(() => {});
    mind?.destroy?.();
  });

  /** A node's link: to a place in a document, else nothing (no page of its own to go to). */
  function click(e) {
    const a = e.target.closest?.("a");
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
    const to = parseSource(a.getAttribute("href") || "");
    if (to && onjump) Promise.resolve(onjump(to)).catch((err) => (error = String(err)));
  }

  function key(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      clearTimeout(timer);
      save();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="mm" onkeydown={key}>
  <div class="bar">
    <strong>{vol.title}</strong>
    <span class="muted">脑图 · 双击节点编辑，Tab 添加子节点，Enter 添加同级节点，右键更多</span>
    <span class="grow"></span>
    <span class="muted">{status}</span>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="map" bind:this={el} onclickcapture={click}></div>
</div>

<style>
  .mm { display: flex; flex-direction: column; height: 100%; }
  .bar { display: flex; gap: 10px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--line); font-size: 13px; }
  .grow { flex: 1; }
  .map { flex: 1; min-height: 0; position: relative; }
  .error { color: var(--hl); padding: 4px 12px; margin: 0; }
</style>
