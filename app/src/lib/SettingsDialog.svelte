<script>
  // Settings: theme, interface zoom and font, reader font, size and line
  // height. Changes apply at once and are kept.
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { settings, update, reset, FONTS, DEFAULT_AI_PRESET, activeAiPreset, toBackendAiConfig } from "./settings.svelte.js";

  let { onclose } = $props();

  const TABS = [
    ["appearance", "外观"],
    ["reader", "阅读与链接"],
    ["search", "搜索与时域"],
    ["dict", "词典"],
    ["daily", "每日一句"],
    ["ai", "AI 助手"],
    ["clean", "数据清理"],
  ];
  let activeTab = $state("appearance");

  // Dictionary settings
  let dictPaths = $state([]);
  let dictDisabled = $state([]);
  let dictList = $state([]);
  let dictFilter = $state("");
  let dictLoading = $state(false);
  let dictMsg = $state("");

  const filteredDicts = $derived.by(() => {
    const q = dictFilter.trim().toLowerCase();
    if (!q) return dictList;
    return dictList.filter((d) => d.name.toLowerCase().includes(q) || d.path.toLowerCase().includes(q));
  });
  const enabledDictCount = $derived(dictList.filter((d) => d.enabled).length);
  const enabledEntriesCount = $derived(dictList.filter((d) => d.enabled).reduce((n, d) => n + (d.size || 0), 0));

  async function loadDictConfig() {
    dictLoading = true;
    dictMsg = "";
    try {
      const st = await api.dictConfigGet();
      dictPaths = st.paths || [];
      dictDisabled = st.disabled || [];
      dictList = st.dicts || [];
    } catch (e) {
      dictMsg = String(e);
    } finally {
      dictLoading = false;
    }
  }

  async function saveDictConfig(nextPaths, nextDisabled) {
    dictLoading = true;
    dictMsg = "";
    try {
      const st = await api.dictConfigSet(nextPaths, nextDisabled);
      dictPaths = st.paths || [];
      dictDisabled = st.disabled || [];
      dictList = st.dicts || [];
    } catch (e) {
      dictMsg = String(e);
    } finally {
      dictLoading = false;
    }
  }

  async function addDictDir() {
    const picked = await openDialog({ directory: true, multiple: false, title: "选择词典目录（将扫描目录下所有 .words 文件）" });
    if (!picked || typeof picked !== "string") return;
    if (dictPaths.includes(picked)) return;
    await saveDictConfig([...dictPaths, picked], dictDisabled);
  }

  async function addDictFile() {
    const picked = await openDialog({
      multiple: true,
      title: "选择词典文件（*.words）",
      filters: [{ name: "词典文件", extensions: ["words"] }],
    });
    if (!picked) return;
    const files = Array.isArray(picked) ? picked : [picked];
    const next = [...dictPaths];
    for (const f of files) {
      if (f && !next.includes(f)) next.push(f);
    }
    await saveDictConfig(next, dictDisabled);
  }

  async function removeDictPath(p) {
    await saveDictConfig(dictPaths.filter((x) => x !== p), dictDisabled);
  }

  async function reloadDicts() {
    dictLoading = true;
    dictMsg = "";
    try {
      const st = await api.dictReload();
      dictPaths = st.paths || [];
      dictDisabled = st.disabled || [];
      dictList = st.dicts || [];
    } catch (e) {
      dictMsg = String(e);
    } finally {
      dictLoading = false;
    }
  }

  async function toggleDict(id) {
    const set = new Set(dictDisabled);
    if (set.has(id)) set.delete(id);
    else set.add(id);
    await saveDictConfig(dictPaths, [...set]);
  }

  async function selectAllDicts() {
    const visible = new Set(filteredDicts.map((d) => d.id));
    const next = dictDisabled.filter((id) => !visible.has(id));
    await saveDictConfig(dictPaths, next);
  }

  async function clearDicts() {
    const set = new Set(dictDisabled);
    for (const d of filteredDicts) set.add(d.id);
    await saveDictConfig(dictPaths, [...set]);
  }

  async function invertDicts() {
    const set = new Set(dictDisabled);
    for (const d of filteredDicts) {
      if (set.has(d.id)) set.delete(d.id);
      else set.add(d.id);
    }
    await saveDictConfig(dictPaths, [...set]);
  }

  // AI presets management & connection test
  let showKey = $state(false);
  let aiTesting = $state(false);
  let aiFetchingModels = $state(false);
  let aiTestMsg = $state("");
  let aiTestErr = $state(false);
  let aiHistoryMsg = $state("");
  let writableLibs = $state([]);
  const curPreset = $derived(activeAiPreset());

  onMount(() => {
    api.status().then((st) => {
      writableLibs = (st.libraries || []).filter(
        (l) => l.kind === "notes" || (l.kind === "folder" && l.writable),
      );
    }).catch(() => {});
    loadDictConfig();
  });

  function patchPreset(field, val) {
    const list = (settings.aiPresets || []).map((p) => (p.id === curPreset.id ? { ...p, [field]: val } : p));
    update({ aiPresets: list });
    aiTestMsg = "";
  }

  function addPreset() {
    const id = `ai-${Date.now()}`;
    const n = (settings.aiPresets?.length || 0) + 1;
    const next = {
      ...DEFAULT_AI_PRESET,
      id,
      name: `模型预设 ${n}`,
      baseUrl: "https://api.openai.com/v1",
      apiKey: "",
      model: "gpt-4o-mini",
      models: [],
      effortByModel: {},
    };
    update({ aiPresets: [...(settings.aiPresets || []), next], aiActiveId: id });
    aiTestMsg = "";
  }

  function removePreset() {
    const list = (settings.aiPresets || []).filter((p) => p.id !== curPreset.id);
    if (!list.length) return;
    update({ aiPresets: list, aiActiveId: list[0].id });
    aiTestMsg = "";
  }

  async function fetchModels(silent = false) {
    const preset = curPreset;
    if (!preset.baseUrl?.trim()) return;
    if (aiFetchingModels) return;
    aiFetchingModels = true;
    if (!silent) {
      aiTestMsg = "";
      aiTestErr = false;
    }
    try {
      const models = await api.aiFetchModels(preset.format || "openai", preset.baseUrl, preset.apiKey || "");
      const list = (settings.aiPresets || []).map((p) => {
        if (p.id !== preset.id) return p;
        const nextModel = p.model?.trim() ? p.model : (models[0] || p.model);
        return { ...p, models, model: nextModel };
      });
      update({ aiPresets: list });
      if (!silent) {
        aiTestErr = false;
        aiTestMsg = models.length ? `已获取并缓存 ${models.length} 个可用模型` : "接口返回了空的模型列表";
      }
    } catch (e) {
      if (!silent) {
        aiTestErr = true;
        aiTestMsg = String(e);
      }
    } finally {
      aiFetchingModels = false;
    }
  }

  function maybeAutoFetchModels() {
    if (curPreset.baseUrl?.trim() && curPreset.apiKey?.trim() && !(curPreset.models?.length)) {
      fetchModels(true);
    }
  }

  async function testAi() {
    aiTesting = true;
    aiTestMsg = "";
    aiTestErr = false;
    try {
      aiTestMsg = await api.aiTest(toBackendAiConfig(curPreset));
      if (!(curPreset.models?.length)) {
        fetchModels(true);
      }
    } catch (e) {
      aiTestErr = true;
      aiTestMsg = String(e);
    } finally {
      aiTesting = false;
    }
  }

  async function clearAiHistory() {
    if (!(await ask("清空全部已保存的 AI 会话历史记录？\n（不会影响任何知识库或笔记文档本身）", { title: "清空 AI 会话历史", kind: "warning", okLabel: "清空", cancelLabel: "取消" }))) return;
    try {
      const n = await api.aiSessionClear();
      aiHistoryMsg = `已清空 ${n} 条会话记录。`;
    } catch (e) {
      aiHistoryMsg = String(e);
    }
  }

  // clearing what the program leaves behind (trash of local folders, pictures nothing refers to)
  let cleaning = $state(false);
  let cleanMsg = $state("");
  async function clean() {
    const note = "会清空读写模式本地文件夹里的 .smart-library/trash（删除的文件和文件夹），并删除没有任何笔记引用的图片。";
    if (!(await ask(`清理缓存？\n${note}\n清空后不能再从回收站找回。`, { title: "清理缓存", kind: "warning", okLabel: "清理", cancelLabel: "取消" }))) return;
    cleaning = true;
    cleanMsg = "";
    try {
      cleanMsg = await api.clearCache();
    } catch (e) {
      cleanMsg = String(e);
    } finally {
      cleaning = false;
    }
  }

  // the table of periods (时域): the user's rows on top of the preset ones
  let dyn = $state(null); // null: closed; otherwise { text, preset, msg }
  let pyDict = $state(null); // null: closed; otherwise { text, msg }
  async function openDyn() {
    try {
      const [user, preset] = await api.dynastyText();
      dyn = { text: user || preset, preset, msg: user ? "" : "显示的是预设表；改动后保存即成为你的表（同名的行覆盖预设）。" };
    } catch (e) {
      dyn = { text: "", preset: "", msg: String(e) };
    }
  }
  async function saveDyn() {
    try {
      const n = await api.setDynastyText(dyn.text);
      dyn.msg = `已保存，${n} 行。已建好的索引要重建后，别称才会合并。`;
    } catch (e) {
      dyn.msg = String(e);
    }
  }
  async function exportDyn() {
    const path = await saveDialog({ defaultPath: "时域表.txt", filters: [{ name: "文本", extensions: ["txt"] }] });
    if (!path) return;
    try {
      await api.saveBytes(path, new TextEncoder().encode(dyn.text));
      dyn.msg = `已导出：${path}`;
    } catch (e) {
      dyn.msg = String(e);
    }
  }
  async function importDyn() {
    const path = await openDialog({ multiple: false, filters: [{ name: "文本", extensions: ["txt", "tsv"] }] });
    if (!path) return;
    try {
      dyn.text = await api.readTextFile(path);
      dyn.msg = "已读入，核对后点「保存」。";
    } catch (e) {
      dyn.msg = String(e);
    }
  }

  async function openPyDict() {
    try {
      const text = await api.pinyinText();
      pyDict = {
        text,
        msg: text ? "" : "当前无自定义条目。格式：词=pin1,yin1（如 阿弥陀佛=e1,mi2,tuo2,fo2），# 开头为注释。",
      };
    } catch (e) {
      pyDict = { text: "", msg: String(e) };
    }
  }
  async function savePyDict() {
    try {
      const n = await api.setPinyinText(pyDict.text);
      pyDict.msg = n ? `已保存 ${n} 条自定义拼音，立即生效。` : "已清空自定义拼音，恢复仅使用默认词库。";
    } catch (e) {
      pyDict.msg = String(e);
    }
  }
  async function exportPyDict() {
    const path = await saveDialog({ defaultPath: "自定义拼音.txt", filters: [{ name: "文本", extensions: ["txt"] }] });
    if (!path) return;
    try {
      await api.saveBytes(path, new TextEncoder().encode(pyDict.text));
      pyDict.msg = `已导出：${path}`;
    } catch (e) {
      pyDict.msg = String(e);
    }
  }
  async function importPyDict() {
    const path = await openDialog({ multiple: false, filters: [{ name: "文本", extensions: ["txt"] }] });
    if (!path) return;
    try {
      pyDict.text = await api.readTextFile(path);
      pyDict.msg = "已读入，核对后点「保存」。";
    } catch (e) {
      pyDict.msg = String(e);
    }
  }

  // the fonts installed on this computer (from the program, not the browser)
  let cjk = $state([]);
  let other = $state([]);
  onMount(() => {
    api
      .listFonts()
      .then((l) => {
        cjk = l.filter((f) => f.cjk);
        other = l.filter((f) => !f.cjk);
      })
      .catch(() => {});
  });
  const css = (f) => f.replace(/["';]/g, "");
  const shown = (f) => (f.label !== f.family ? `${f.label}（${f.family}）` : f.family);
</script>

{#snippet picker(field, customField, hint)}
  <select value={settings[field]} onchange={(e) => update({ [field]: e.currentTarget.value })}>
    {#each FONTS.filter((f) => f[0] !== "custom") as [v, label]}<option value={v}>{label}</option>{/each}
    {#if cjk.length}
      <optgroup label="本机中文字体">
        {#each cjk as f (f.family)}<option value={"sys:" + f.family} style="font-family: '{css(f.family)}'">{shown(f)}</option>{/each}
      </optgroup>
    {/if}
    {#if other.length}
      <optgroup label="本机其他字体">
        {#each other as f (f.family)}<option value={"sys:" + f.family} style="font-family: '{css(f.family)}'">{shown(f)}</option>{/each}
      </optgroup>
    {/if}
    <option value="custom">手动输入字体名…</option>
  </select>
  {#if settings[field] === "custom"}
    <input type="text" placeholder={hint} value={settings[customField]}
      onchange={(e) => update({ [customField]: e.currentTarget.value })} />
  {/if}
{/snippet}

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="设置" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>设置</h2></header>
    <div class="dialog-content settings-split">
      <nav class="tabs" aria-label="设置分组">
        {#each TABS as [id, label] (id)}
          <button type="button" class="tab-btn" class:on={activeTab === id} onclick={() => (activeTab = id)}>{label}</button>
        {/each}
      </nav>
      <div class="tab-body">
        {#if activeTab === "appearance"}
          <div class="sec-head">
            <h3>外观</h3>
            <p class="muted">设置界面主题、缩放比例与界面字体。</p>
          </div>
          <div class="card">
            <div class="row">
              <span>主题</span>
              <div class="seg" role="group" aria-label="主题">
                {#each [["auto", "跟随系统"], ["light", "浅色"], ["dark", "深色"]] as [v, label]}
                  <button class:on={settings.theme === v} onclick={() => update({ theme: v })}>{label}</button>
                {/each}
              </div>
            </div>
            <div class="row">
              <span>界面缩放</span>
              <input type="range" min="80" max="150" step="5" value={settings.uiZoom}
                oninput={(e) => update({ uiZoom: Number(e.currentTarget.value) })} />
              <output>{settings.uiZoom}%</output>
            </div>
            <div class="row">
              <span>界面字体</span>
              {@render picker("uiFont", "uiFontCustom", "字体名称，如 Microsoft YaHei")}
            </div>
          </div>

        {:else if activeTab === "reader"}
          <div class="sec-head">
            <h3>阅读与链接</h3>
            <p class="muted">设置阅读器排版样式，以及所有编辑器和阅读器中外部链接的打开方式。</p>
          </div>
          <div class="card">
            <div class="row">
              <span>字体</span>
              {@render picker("readerFont", "readerFontCustom", "字体名称，如 KaiTi")}
            </div>
            <div class="row">
              <span>字号</span>
              <input type="range" min="14" max="34" step="1" value={settings.readerSize}
                oninput={(e) => update({ readerSize: Number(e.currentTarget.value) })} />
              <output>{settings.readerSize} px</output>
            </div>
            <div class="row">
              <span>行距</span>
              <input type="range" min="1.4" max="2.6" step="0.1" value={settings.readerLine}
                oninput={(e) => update({ readerLine: Number(e.currentTarget.value) })} />
              <output>{settings.readerLine}</output>
            </div>
            <div class="preview-wrap">
              <p class="sample" aria-label="预览">如是我聞。一時佛在舍衛國祇樹給孤獨園。Sample 0123</p>
            </div>
          </div>
          <div class="card">
            <div class="row">
              <span>外部链接</span>
              <select value={settings.externalLink} onchange={(e) => update({ externalLink: e.currentTarget.value })}>
                <option value="window">在新窗口中打开（默认）</option>
                <option value="ask">每次询问</option>
                <option value="browser">在系统浏览器中打开</option>
              </select>
            </div>
          </div>

        {:else if activeTab === "search"}
          <div class="sec-head">
            <h3>搜索与时域</h3>
            <p class="muted">调整标题快搜加载数量，以及管理索引与检索使用的朝代时域表。</p>
          </div>
          <div class="card">
            <div class="row">
              <span>快搜每批</span>
              <input type="range" min="50" max="1000" step="50" value={settings.quickLimit}
                oninput={(e) => update({ quickLimit: Number(e.currentTarget.value) })} />
              <output>{settings.quickLimit} 项</output>
            </div>
            <p class="muted hint card-note">标题快搜先显示这么多条，列表滚到底（或用键盘走到末尾）时自动再加载一批。</p>
            <label class="check row-top">
              <input type="checkbox" checked={settings.quickPinyin}
                onchange={(e) => update({ quickPinyin: e.currentTarget.checked })} />
              <span>标题快搜支持拼音匹配（可输入全拼或首字母，如 <code>miaofa</code> 或 <code>mflhj</code>）</span>
            </label>
          </div>
          <div class="card">
            <div class="card-block">
              <div class="block-title">时域表</div>
              <p class="muted hint">朝代的名称、起止年份和别称（别称在索引时并入第一列的名称，搜索页的「时域」按起年排序）。可导出、导入。</p>
              {#if !dyn}
                <div class="row-plain"><button onclick={openDyn}>编辑时域表…</button></div>
              {:else}
                <textarea class="dyn" bind:value={dyn.text} spellcheck="false" aria-label="时域表"></textarea>
                <div class="row-plain">
                  <button onclick={saveDyn}>保存</button>
                  <button onclick={() => (dyn.text = dyn.preset)}>恢复预设</button>
                  <button onclick={importDyn}>导入…</button>
                  <button onclick={exportDyn}>导出…</button>
                  <button onclick={() => (dyn = null)}>收起</button>
                </div>
                {#if dyn.msg}<p class="muted hint">{dyn.msg}</p>{/if}
              {/if}
            </div>
          </div>
          <div class="card">
            <div class="card-block">
              <div class="block-title">自定义拼音词典</div>
              <p class="muted hint">在内置词库之外增补或覆盖字词读音（先加载默认词库，再加载此处条目）。</p>
              {#if !pyDict}
                <div class="row-plain"><button onclick={openPyDict}>编辑自定义拼音…</button></div>
              {:else}
                <textarea class="dyn" bind:value={pyDict.text} spellcheck="false" aria-label="自定义拼音词典"
                  placeholder={"# 示例：\n阿弥陀经=e1,mi2,tuo2,jing1\n般若=bo1,re3"}></textarea>
                <div class="row-plain">
                  <button onclick={savePyDict}>保存</button>
                  <button onclick={() => (pyDict.text = "")}>清空</button>
                  <button onclick={importPyDict}>导入…</button>
                  <button onclick={exportPyDict}>导出…</button>
                  <button onclick={() => (pyDict = null)}>收起</button>
                </div>
                {#if pyDict.msg}<p class="muted hint">{pyDict.msg}</p>{/if}
              {/if}
            </div>
          </div>

        {:else if activeTab === "dict"}
          <div class="sec-head">
            <h3>词典配置</h3>
            <p class="muted">添加 .words 词典文件或目录（自动扫描子目录），并选择默认启用的词典。</p>
          </div>
          <div class="card">
            <div class="card-block">
              <div class="block-title">词库路径</div>
              <div class="row-plain">
                <button type="button" onclick={addDictDir} disabled={dictLoading}>添加目录…</button>
                <button type="button" onclick={addDictFile} disabled={dictLoading}>添加文件 (*.words)…</button>
                <button type="button" onclick={reloadDicts} disabled={dictLoading}>{dictLoading ? "扫描中…" : "重新扫描"}</button>
              </div>
              {#if !dictPaths.length}
                <p class="muted hint">尚未添加词典目录或文件。</p>
              {:else}
                <div class="dict-paths">
                  {#each dictPaths as p (p)}
                    <div class="src">
                      <span class="lab" title={p}>{p}</span>
                      <button type="button" class="link" onclick={() => removeDictPath(p)}>移除</button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
          <div class="card">
            <div class="card-block">
              <div class="dict-list-head">
                <div class="block-title" style="margin:0">
                  可加载词典（已启用 {enabledDictCount}/{dictList.length}，共 {enabledEntriesCount.toLocaleString()} 词条）
                </div>
                <div class="row-plain" style="margin:0">
                  <button type="button" onclick={selectAllDicts} disabled={!filteredDicts.length || dictLoading}>全选</button>
                  <button type="button" onclick={clearDicts} disabled={!filteredDicts.length || dictLoading}>清空</button>
                  <button type="button" onclick={invertDicts} disabled={!filteredDicts.length || dictLoading}>反选</button>
                </div>
              </div>
              {#if dictList.length > 6}
                <div class="row-plain">
                  <input type="text" bind:value={dictFilter} placeholder="过滤词典名称或路径…" style="flex:1; min-width:160px" />
                </div>
              {/if}
              {#if !dictList.length}
                <p class="muted hint" style="margin-top:8px">未发现 .words 词典文件。</p>
              {:else}
                <div class="dict-items">
                  {#each filteredDicts as d (d.id)}
                    <label class="dict-item" title={d.path}>
                      <input type="checkbox" checked={d.enabled} onchange={() => toggleDict(d.id)} />
                      <span class="dict-name">{d.name}</span>
                      {#if d.has_extra}<span class="dict-badge">图</span>{/if}
                      <span class="muted dict-count">{d.size.toLocaleString()} 条</span>
                    </label>
                  {/each}
                </div>
              {/if}
              {#if dictMsg}<p class="hint err-text">{dictMsg}</p>{/if}
            </div>
          </div>

        {:else if activeTab === "daily"}
          <div class="sec-head">
            <h3>每日一句</h3>
            <p class="muted">每天从知识库中随机抽取一句，在目录树下方提示。</p>
          </div>
          <div class="card">
            <label class="row">
              <input type="checkbox" checked={settings.dailyOn} onchange={(e) => update({ dailyOn: e.currentTarget.checked })} />
              <span class="wide">启用每日一句提示（在目录树下方的喇叭按钮上显示）</span>
            </label>
            <div class="row" style="align-items: flex-start">
              <span style="padding-top: 2px">来源</span>
              <div class="sources">
                {#each settings.dailySources as src, i}
                  <div class="src" title={src.label}>
                    <span class="lab">{src.label}</span>
                    <button class="link" onclick={() => update({ dailySources: settings.dailySources.filter((_, j) => j !== i) })} aria-label="移除">×</button>
                  </div>
                {:else}
                  <span class="muted">全部知识库</span>
                {/each}
              </div>
            </div>
            <p class="muted hint card-note">在目录树里右键知识库或节点，选「加入每日一句来源」；没有来源时从全部已建索引的知识库里抽。</p>
          </div>

        {:else if activeTab === "ai"}
          <div class="sec-head">
            <h3>AI 助手</h3>
            <p class="muted">配置大语言模型接口预设、默认提示词与 AI 笔记保存位置。</p>
          </div>
          <div class="card">
            <label class="row">
              <input type="checkbox" checked={settings.aiOn} onchange={(e) => update({ aiOn: e.currentTarget.checked })} />
              <span class="wide">启用 AI 辅助功能（在阅读器、笔记/脑图/白板编辑器、搜索结果旁显示 AI 助手，开启笔记内置 AI 与独立会话）</span>
            </label>
            <div class="row">
              <span>当前预设</span>
              <select value={curPreset.id} onchange={(e) => { update({ aiActiveId: e.currentTarget.value }); aiTestMsg = ""; }}>
                {#each settings.aiPresets || [] as p (p.id)}
                  <option value={p.id}>{p.name}（{p.model || "未设模型"}）</option>
                {/each}
              </select>
              <button type="button" onclick={addPreset}>＋新增预设</button>
              {#if (settings.aiPresets?.length || 0) > 1}
                <button type="button" onclick={removePreset}>删除此预设</button>
              {/if}
            </div>
            <div class="row">
              <span>预设名称</span>
              <input type="text" value={curPreset.name} placeholder="如 DeepSeek / 本地 Ollama / Claude"
                oninput={(e) => patchPreset("name", e.currentTarget.value)} />
            </div>
            <div class="row">
              <span>接口协议</span>
              <div class="seg" role="group" aria-label="接口协议">
                {#each [["openai", "OpenAI 兼容接口"], ["anthropic", "Anthropic 原生"]] as [v, label]}
                  <button type="button" class:on={curPreset.format === v} onclick={() => patchPreset("format", v)}>{label}</button>
                {/each}
              </div>
            </div>
            <div class="row">
              <span>接口地址</span>
              <input type="text" value={curPreset.baseUrl}
                placeholder={curPreset.format === "anthropic" ? "https://api.anthropic.com" : "https://api.deepseek.com/v1 或 http://localhost:11434/v1"}
                oninput={(e) => patchPreset("baseUrl", e.currentTarget.value)}
                onblur={maybeAutoFetchModels} />
            </div>
            <div class="row">
              <span>API Key</span>
              <input type={showKey ? "text" : "password"} value={curPreset.apiKey} placeholder="本地免密模型（如 Ollama）可留空"
                oninput={(e) => patchPreset("apiKey", e.currentTarget.value)}
                onblur={maybeAutoFetchModels} />
              <button type="button" class="link" onclick={() => (showKey = !showKey)}>{showKey ? "隐藏" : "显示"}</button>
            </div>
            <div class="row">
              <span>模型名称</span>
              <input type="text" list="ai-models-list" value={curPreset.model} placeholder="如 deepseek-chat、qwen-plus、gpt-4o、claude-3-7-sonnet-latest"
                oninput={(e) => patchPreset("model", e.currentTarget.value)} />
              <datalist id="ai-models-list">
                {#each curPreset.models || [] as m (m)}
                  <option value={m}></option>
                {/each}
              </datalist>
              <button type="button" onclick={() => fetchModels(false)} disabled={aiFetchingModels || !curPreset.baseUrl?.trim()} title="从接口获取可用模型列表并缓存">
                {aiFetchingModels ? "获取中…" : "刷新模型"}
              </button>
            </div>
            <div class="row" style="align-items: flex-start">
              <span style="padding-top: 6px">系统提示</span>
              <textarea class="ai-sys" value={curPreset.systemPrompt} placeholder="设定 AI 助手的角色与回答风格…"
                oninput={(e) => patchPreset("systemPrompt", e.currentTarget.value)}></textarea>
            </div>
            <div class="row">
              <span></span>
              <button type="button" onclick={testAi} disabled={aiTesting}>{aiTesting ? "测试中…" : "测试连接"}</button>
              <button type="button" onclick={clearAiHistory}>清空 AI 会话历史</button>
            </div>
            {#if aiTestMsg}
              <p class="hint card-note" class:err-text={aiTestErr}>{aiTestMsg}</p>
            {/if}
            {#if aiHistoryMsg}
              <p class="muted hint card-note">{aiHistoryMsg}</p>
            {/if}
          </div>
          <div class="card">
            <div class="row">
              <span>新笔记保存</span>
              <div class="seg" role="group" aria-label="新笔记保存方式">
                {#each [["ask", "每次弹窗选择（推荐）"], ["default", "直接存到默认位置"]] as [v, label]}
                  <button type="button" class:on={(settings.aiSaveMode || "ask") === v} onclick={() => update({ aiSaveMode: v })}>{label}</button>
                {/each}
              </div>
            </div>
            {#if settings.aiSaveMode === "default" || settings.aiSaveLib || settings.aiSavePath}
              <div class="row">
                <span>默认知识库</span>
                <select value={settings.aiSaveLib || ""} onchange={(e) => update({ aiSaveLib: e.currentTarget.value })}>
                  <option value="">自动选择第一个可写库</option>
                  {#each writableLibs as l (l.id)}
                    <option value={l.id}>{l.title}（{l.kind === "folder" ? "本地文件夹" : "内部笔记"}）</option>
                  {/each}
                </select>
              </div>
              <div class="row">
                <span>默认路径</span>
                <input type="text" value={settings.aiSavePath || ""} placeholder="留空表示根目录，例如：AI 笔记/读书整理"
                  oninput={(e) => update({ aiSavePath: e.currentTarget.value })} />
              </div>
            {/if}
          </div>

        {:else if activeTab === "clean"}
          <div class="sec-head">
            <h3>数据清理</h3>
            <p class="muted">清理本地回收站及未被任何笔记引用的孤立图片资源。</p>
          </div>
          <div class="card">
            <div class="card-block">
              <div class="row-plain">
                <button onclick={clean} disabled={cleaning}>{cleaning ? "清理中…" : "清理缓存"}</button>
              </div>
              <p class="muted hint">清空本地文件夹（读写模式）里的回收站 .smart-library/trash，并删除没有笔记引用的图片（内部知识库也一样）。</p>
              {#if cleanMsg}<p class="hint" style="white-space: pre-line">{cleanMsg}</p>{/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
    <div class="actions">
      <button onclick={reset}>恢复默认</button>
      <button class="primary" onclick={onclose}>完成</button>
    </div>
  </div>
</div>

<style>
  .dyn { width: 100%; height: 160px; font-family: monospace; font-size: 12px; box-sizing: border-box; margin: 6px 0; }
  .ai-sys { flex: 1; min-width: 200px; height: 68px; font-family: inherit; font-size: 13px; box-sizing: border-box; resize: vertical; }
  .err-text { color: var(--hl); }
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(760px, calc(100vw - 32px)); height: min(580px, calc(100vh - 48px)); }
  h2 { margin: 0; font-size: 20px; color: var(--accent); }
  .settings-split {
    display: flex; flex: 1; min-height: 0; padding: 0; overflow: hidden;
  }
  .tabs {
    width: 156px; flex: none; padding: 12px 10px; border-right: 1px solid var(--line);
    background: var(--bg); display: flex; flex-direction: column; gap: 4px; overflow-y: auto;
  }
  .tab-btn {
    display: block; width: 100%; text-align: left; border: none; background: transparent;
    padding: 8px 12px; border-radius: 7px; font-size: 14px; color: var(--ink); cursor: pointer;
  }
  .tab-btn:hover { background: var(--accent-soft); }
  .tab-btn.on { background: var(--paper); color: var(--accent); font-weight: 600; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08); }
  .tab-body {
    flex: 1; min-width: 0; min-height: 0; overflow-y: auto; padding: 18px 22px; overscroll-behavior: contain;
  }
  .sec-head { margin: 0 0 14px; }
  .sec-head h3 { margin: 0 0 4px; font-size: 18px; color: var(--ink); }
  .sec-head p { margin: 0; font-size: 13px; }
  .card {
    border: 1px solid var(--line); border-radius: 10px; background: var(--paper);
    margin-bottom: 14px; overflow: hidden;
  }
  .card .row {
    display: flex; gap: 12px; align-items: center; padding: 10px 14px; margin: 0; flex-wrap: wrap;
    border-bottom: 1px solid var(--line);
  }
  .card .row:last-child { border-bottom: none; }
  .card-block { padding: 12px 14px; }
  .block-title { font-weight: 600; margin-bottom: 4px; }
  .row-plain { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 8px 0; }
  .card-note { padding: 8px 14px 10px; margin: 0; border-top: 1px solid var(--line); }
  .preview-wrap { padding: 10px 14px; border-top: 1px solid var(--line); }
  .row > span { width: 78px; flex: none; }
  .row input[type="range"] { flex: 1; min-width: 140px; }
  .row input[type="text"], .row input[type="password"] { flex: 1; min-width: 160px; }
  .row select { max-width: 340px; }
  output { width: 56px; text-align: right; color: var(--muted); }
  .seg { display: flex; }
  .seg button { border-radius: 0; margin-left: -1px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 6px 6px 0; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .sample {
    margin: 0; padding: 10px 14px; background: var(--bg); border-radius: 6px;
    font-family: var(--reader-font, inherit); font-size: var(--reader-size, 19px); line-height: var(--reader-line, 2);
  }
  .row > span.wide, .wide { width: auto; flex: 1 1 0%; min-width: 0; white-space: normal; line-height: 1.5; }
  .sources { flex: 1; min-width: 0; }
  .src { display: flex; align-items: center; gap: 6px; padding: 2px 0; border-bottom: 1px dashed var(--line); }
  .src .lab { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { margin: 0 0 6px; font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
  .dict-paths { max-height: 120px; overflow-y: auto; margin-top: 6px; }
  .dict-list-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }
  .dict-items { max-height: 240px; overflow-y: auto; border: 1px solid var(--line); border-radius: 6px; padding: 4px 8px; background: var(--bg); }
  .dict-item { display: flex; align-items: center; gap: 8px; padding: 4px 2px; border-bottom: 1px dashed var(--line); cursor: pointer; font-size: 13px; }
  .dict-item:last-child { border-bottom: none; }
  .dict-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dict-badge { font-size: 11px; padding: 0 4px; border-radius: 4px; background: var(--accent-soft); color: var(--accent); flex: none; }
  .dict-count { font-size: 12px; flex: none; }
</style>
