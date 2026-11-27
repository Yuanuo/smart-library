<script>
  // AI assistant panel: used beside any reader/editor (CBETA, HTML, ZIM,
  // Markdown note, mind map, whiteboard), beside full-text search results,
  // or as a standalone chat tab / window.
  //
  // Features:
  //  - Switch between multiple user-configured model presets on the fly;
  //  - Reference one or more documents from any bound library (`+ 引用文档`,
  //    typing `@` in the input box, or dragging nodes from the library tree);
  //  - Persist chat history in `ai_chat.db` (associated with `(lib, path)` for
  //    documents, never modifying the document itself), with restore & delete;
  //  - Interact with and directly edit writable documents (Markdown notes,
  //    Mind-Elixir mind maps, Excalidraw whiteboards);
  //  - Generate innovation plans or syntheses from full-text search results.
  import { onMount, onDestroy, tick, untrack } from "svelte";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { emit } from "@tauri-apps/api/event";
  import * as api from "./api.js";
  import Icon from "./Icon.svelte";
  import AiHistoryDialog from "./AiHistoryDialog.svelte";
  import AiSaveNoteDialog from "./AiSaveNoteDialog.svelte";
  import { settings, update as updateSettings, activeAiPreset, toBackendAiConfig, AI_PROVIDER_TEMPLATES, isAiConfigured, isLocalAiUrl } from "./settings.svelte.js";
  import { formatShortcut, formatCombo } from "./shortcuts.js";

  let {
    // "doc" | "search" | "chat"
    scopeKind = "doc",
    // For "doc": { lib, path, bookId, title, contentType, writable, getText }
    doc = null,
    // For "search": { query, hits, totalVolumes, totalBooks }
    searchContext = null,
    // Selected text sent from reader/editor: { text, action, nonce }
    incoming = null,
    // Callback to edit the current writable document: ({ mode: "replace"|"append", text }) => Promise<void>|void
    onEditDoc = null,
    // Callback to open a document in workspace
    onOpenDoc = null,
    // Callback to pop out standalone chat
    onPopout = null,
    // Callback to close side panel
    onClose = null,
    // Optional initial session id for standalone chat tab / window
    initialSessionId = null,
    // Optional callback when session id or title changes: ({ sessionId, title }) => void
    onSessionChange = null,
    // Callback when an AI agent tool mutates a library/document: (lib, path) => void
    onLibraryChanged = null,
    // Callback to open settings dialog on a specific tab (e.g. "ai" | "mcp")
    onOpenSettings = null,
  } = $props();

  let sessionId = $state(null);
  let sessionTitle = $state("");
  let customTitled = $state(false);
  let messages = $state([]); // [{ role: "user"|"assistant", content, reasoning?, toolLogs?, ts }]
  let refs = $state([]); // [{ lib, bookId, path, title, libTitle? }]
  let quotes = $state([]); // [{ text, source }] selected passages from reader/editor/dict
  let includeDoc = $state(true);
  let includeSearch = $state(true);
  let input = $state("");
  let inputEl = $state();
  let listEl = $state();
  let busy = $state(false);
  let activeReqId = $state(null);
  let statusMsg = $state("");
  let errorMsg = $state("");

  const EFFORTS = [
    ["", "推理：默认"],
    ["low", "推理：低 (low)"],
    ["medium", "推理：中 (medium)"],
    ["high", "推理：高 (high)"],
    ["xhigh", "推理：极高 (xhigh)"],
    ["ultra", "推理：超强 (ultra)"],
    ["max", "推理：最大 (max)"],
  ];
  let reasoningEffort = $state("");
  try {
    const initialPreset = activeAiPreset();
    const saved = initialPreset?.effortByModel?.[initialPreset?.model || ""];
    reasoningEffort = typeof saved === "string" ? saved : (localStorage.getItem("ai.reasoningEffort") || "");
  } catch {}
  function setEffort(v) {
    reasoningEffort = v;
    try {
      localStorage.setItem("ai.reasoningEffort", v);
    } catch {}
    const m = curPreset?.model?.trim();
    if (m) {
      const list = (settings.aiPresets || []).map((p) =>
        p.id === curPreset.id
          ? { ...p, effortByModel: { ...(p.effortByModel || {}), [m]: v } }
          : p,
      );
      updateSettings({ aiPresets: list });
    }
  }

  const modelChoices = $derived.by(() => {
    const cached = Array.isArray(curPreset?.models) ? curPreset.models : [];
    const cur = curPreset?.model?.trim();
    if (cur && !cached.includes(cur)) return [cur, ...cached];
    return cached;
  });

  function selectPreset(id) {
    updateSettings({ aiActiveId: id });
    const next = (settings.aiPresets || []).find((p) => p.id === id);
    if (next) {
      const saved = next.effortByModel?.[next.model];
      reasoningEffort = typeof saved === "string" ? saved : "";
    }
  }

  function selectModel(model) {
    const list = (settings.aiPresets || []).map((p) =>
      p.id === curPreset.id ? { ...p, model } : p,
    );
    updateSettings({ aiPresets: list });
    const saved = curPreset?.effortByModel?.[model];
    reasoningEffort = typeof saved === "string" ? saved : "";
  }

  // History modal dialog state
  let showHistory = $state(false);

  // Document reference picker (`@` or `+ 引用文档`)
  let showPicker = $state(false);
  let pickerQuery = $state("");
  let pickerResults = $state([]);
  let pickerBusy = $state(false);
  let pickerTimer = null;
  let pickerInputEl = $state();

  // Cache of fetched document texts (`lib|bookId|path` -> { title, text })
  const docCache = new Map();

  const curPreset = $derived(activeAiPreset());
  const aiConfigured = $derived(isAiConfigured(curPreset));

  // Quick setup form state (shown inline when AI is not configured yet)
  let setupTpl = $state("deepseek");
  let setupFormat = $state("openai");
  let setupBaseUrl = $state("");
  let setupApiKey = $state("");
  let setupModel = $state("");
  let setupShowKey = $state(false);
  let setupTesting = $state(false);
  let setupMsg = $state("");
  let setupErr = $state(false);
  let setupSyncedId = "";

  $effect(() => {
    const p = curPreset;
    if (!p || p.id === setupSyncedId) return;
    setupSyncedId = p.id;
    setupFormat = p.format || "openai";
    setupBaseUrl = p.baseUrl || "";
    setupApiKey = p.apiKey || "";
    setupModel = p.model || "";
    const matched = AI_PROVIDER_TEMPLATES.find((t) => t.baseUrl === p.baseUrl);
    setupTpl = matched ? matched.id : "";
  });

  function applySetupTemplate(tpl) {
    setupTpl = tpl.id;
    setupFormat = tpl.format;
    setupBaseUrl = tpl.baseUrl;
    setupModel = tpl.model;
    setupMsg = "";
    setupErr = false;
  }

  function saveQuickSetup() {
    const url = setupBaseUrl.trim();
    const model = setupModel.trim();
    const key = setupApiKey.trim();
    if (!url || !model) {
      setupErr = true;
      setupMsg = "请填写接口地址和模型名称。";
      return false;
    }
    if (!isLocalAiUrl(url) && !key) {
      setupErr = true;
      setupMsg = "远程服务需要填写 API Key（本地 Ollama 可留空）。";
      return false;
    }
    const tpl = AI_PROVIDER_TEMPLATES.find((t) => t.id === setupTpl);
    const list = (settings.aiPresets || []).map((p) =>
      p.id === curPreset.id
        ? {
            ...p,
            name: tpl && p.id === "default-openai" ? tpl.label : p.name,
            format: setupFormat,
            baseUrl: url,
            apiKey: key,
            model,
          }
        : p,
    );
    updateSettings({ aiOn: true, aiPresets: list });
    setupErr = false;
    setupMsg = "";
    statusMsg = "AI 模型已就绪，可以直接开始对话。";
    return true;
  }

  async function testAndSaveQuickSetup() {
    if (!saveQuickSetup()) return;
    setupTesting = true;
    setupMsg = "正在测试连接…";
    setupErr = false;
    try {
      const reply = await api.aiTest(toBackendAiConfig(activeAiPreset()));
      setupMsg = `连接成功：${reply || "OK"}`;
    } catch (e) {
      setupErr = true;
      setupMsg = `连接失败：${e}`;
    } finally {
      setupTesting = false;
    }
  }
  const docKindLabel = $derived(
    doc?.contentType === "mindmap"
      ? "脑图"
      : doc?.contentType === "whiteboard"
        ? "白板"
        : doc?.writable
          ? "笔记"
          : "文档",
  );

  function renderMd(md) {
    if (!md) return "";
    try {
      const raw = marked.parse(md, { breaks: true, gfm: true });
      return DOMPurify.sanitize(raw);
    } catch {
      return md.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    }
  }

  function scrollBottom() {
    tick().then(() => {
      if (listEl) listEl.scrollTop = listEl.scrollHeight;
    });
  }

  // ---- Load latest session when bound to a document, or initialSessionId for standalone chat
  let loadedDocKey = "";
  let loadedInitId = null;
  $effect(() => {
    const key = scopeKind === "doc" && doc?.lib && doc?.path ? `${doc.lib}|${doc.path}` : "";
    if (key && key !== loadedDocKey) {
      loadedDocKey = key;
      untrack(() => loadLatestDocSession(doc.lib, doc.path));
    } else if (scopeKind === "chat" && initialSessionId && initialSessionId !== loadedInitId) {
      loadedInitId = initialSessionId;
      untrack(() => {
        api.aiSessionGet(initialSessionId).then((s) => s && applyLoadedSession(s)).catch(() => {});
      });
    }
  });

  async function loadLatestDocSession(lib, path) {
    try {
      const s = await api.aiSessionLatest(lib, path);
      if (s) applyLoadedSession(s);
      else {
        sessionId = null;
        messages = [];
        refs = [];
      }
    } catch {}
  }

  function applyLoadedSession(s) {
    sessionId = s.id;
    sessionTitle = s.title || "";
    customTitled = !!s.title;
    try {
      messages = JSON.parse(s.messages_json || "[]");
    } catch {
      messages = [];
    }
    try {
      refs = JSON.parse(s.refs_json || "[]");
    } catch {
      refs = [];
    }
    errorMsg = "";
    statusMsg = "";
    onSessionChange?.({ sessionId: s.id, title: s.title || "AI 助手" });
    scrollBottom();
  }

  function fallbackTitle(text) {
    const clean = (text || "")
      .replace(/^>\s.*$/gm, "")
      .replace(/[#*`>\-\s]+/g, " ")
      .trim();
    const clause = clean.split(/[，。？！；\n,.?!;]/)[0]?.trim() || clean;
    return clause.slice(0, 16) || "新会话";
  }

  async function persistSession() {
    if (!messages.length) return;
    const firstUser = messages.find((m) => m.role === "user")?.content || "";
    const cleanTitle = (sessionTitle || "").trim() || fallbackTitle(firstUser);
    const resolvedKind = scopeKind === "unified" ? (doc ? "doc" : searchContext ? "search" : "chat") : scopeKind;
    try {
      const id = await api.aiSessionSave({
        id: sessionId,
        scopeKind: resolvedKind,
        lib: doc?.lib || "",
        path: doc?.path || "",
        bookId: doc?.bookId || null,
        title: cleanTitle,
        docTitle: doc?.title || (searchContext ? `搜索：${searchContext?.query || "全部"}` : "工作区会话"),
        messagesJson: JSON.stringify(messages),
        refsJson: JSON.stringify(refs),
      });
      sessionId = id;
      if (!sessionTitle) sessionTitle = cleanTitle;
      onSessionChange?.({ sessionId: id, title: cleanTitle });
    } catch (e) {
      console.warn("save ai session", e);
    }
  }

  async function selectSession(id) {
    try {
      const s = await api.aiSessionGet(id);
      if (s) applyLoadedSession(s);
    } catch (e) {
      errorMsg = String(e);
    }
  }

  function newSession() {
    if (busy) stopStream();
    sessionId = null;
    sessionTitle = "";
    customTitled = false;
    messages = [];
    quotes = [];
    errorMsg = "";
    statusMsg = "";
    onSessionChange?.({ sessionId: null, title: "AI 助手" });
    inputEl?.focus();
  }

  // ---- Incoming selection / quick command from Reader or Editor
  let seenIncoming = 0;
  $effect(() => {
    const inc = incoming;
    if (!inc || !inc.nonce || inc.nonce === seenIncoming) return;
    seenIncoming = inc.nonce;
    untrack(() => {
      if (inc.sessionId) {
        api.aiSessionGet(inc.sessionId).then((s) => s && applyLoadedSession(s)).catch(() => {});
      }
      if (inc.sourceDoc?.lib && (inc.sourceDoc?.path || inc.sourceDoc?.bookId)) {
        addRefItem(inc.sourceDoc);
      }
      if (inc.text && inc.text.trim()) {
        const item = { text: inc.text.trim(), source: (inc.source || "").trim() };
        quotes = inc.append ? [...quotes, item] : [item];
      }
      if (inc.prompt) {
        sendPrompt(inc.prompt);
      } else if (!inc.sessionId) {
        inputEl?.focus();
      }
    });
  });

  // ---- Reference Picker (`+ 引用文档` or `@` trigger, plus tree drag-drop)
  function openPicker(initial = "") {
    showPicker = true;
    pickerQuery = initial;
    searchPicker(initial);
    tick().then(() => pickerInputEl?.focus());
  }

  function onPickerInput() {
    clearTimeout(pickerTimer);
    pickerTimer = setTimeout(() => searchPicker(pickerQuery), 180);
  }

  async function searchPicker(q) {
    pickerBusy = true;
    try {
      const res = await api.quickSearch(q.trim(), null, 30);
      pickerResults = (res.items || []).filter((it) => it.kind !== "folder");
    } catch {
      pickerResults = [];
    } finally {
      pickerBusy = false;
    }
  }

  function addRefItem(it) {
    const lib = it.lib;
    const bookId = it.book_id || it.bookId || null;
    const path = it.path || "";
    const key = `${lib}|${bookId || ""}|${path}`;
    if (!refs.some((r) => `${r.lib}|${r.bookId || ""}|${r.path}` === key)) {
      refs = [
        ...refs,
        {
          lib,
          bookId,
          path,
          title: it.title || it.label || path || bookId,
          libTitle: it.lib_title || it.libTitle || "",
        },
      ];
    }
    showPicker = false;
    // If input had a trailing `@`, remove it
    if (input.endsWith("@")) input = input.slice(0, -1);
    inputEl?.focus();
  }

  function removeRef(i) {
    refs = refs.filter((_, idx) => idx !== i);
  }

  /** Handle nodes dragged from the left library tree (`data-scope-drop`). */
  function onScopeDrop(e) {
    const items = e.detail || [];
    for (const it of items) {
      if (it.books?.length) {
        for (const b of it.books) {
          addRefItem({ lib: it.lib, book_id: b, path: "", title: it.label });
        }
      } else if (it.paths?.length) {
        for (const p of it.paths) {
          addRefItem({ lib: it.lib, book_id: null, path: p, title: it.label });
        }
      } else if (it.rels?.length) {
        for (const r of it.rels) {
          addRefItem({ lib: it.lib, book_id: null, path: r, title: it.label });
        }
      }
    }
  }

  function onInputKey(e) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendPrompt();
    } else if (e.key === "@" && !showPicker) {
      setTimeout(() => {
        if (input.endsWith("@")) openPicker("");
      }, 0);
    }
  }

  // ---- Build context & stream LLM response
  function buildSystemPrompt() {
    const base = (curPreset.systemPrompt || "").trim() || "你是 Smart Library 知识库的 AI 助手。";
    const extras = [];
    if (doc?.contentType === "mindmap") {
      extras.push(
        "当前激活标签是一篇「脑图（MindMap）」文档。当用户请你生成、绘制或修改脑图结构时，请输出清晰的 Markdown 缩进列表大纲（如 `- 中心主题\\n  - 分支一\\n    - 子节点`），以便一键应用到脑图画布。",
      );
    } else if (doc?.contentType === "whiteboard") {
      extras.push(
        `当前激活标签是一篇「白板（Excalidraw）」文档。当用户请你绘制流程图、架构图、概念图或白板内容时，请在回复中包含一个 \`\`\`excalidraw 代码块，内容为 JSON 数组，格式如下：
\`\`\`excalidraw
[
  { "id": "n1", "type": "rectangle", "x": 80, "y": 100, "width": 170, "height": 68, "backgroundColor": "#e7f5ff", "strokeColor": "#1971c2", "label": { "text": "核心概念" } },
  { "id": "n2", "type": "rectangle", "x": 340, "y": 100, "width": 170, "height": 68, "backgroundColor": "#ebfbee", "strokeColor": "#2f9e44", "label": { "text": "关键步骤" } },
  { "type": "arrow", "x": 250, "y": 134, "width": 90, "height": 0, "start": { "id": "n1" }, "end": { "id": "n2" }, "label": { "text": "推进" } }
]
\`\`\`
支持图形 type: "rectangle" | "ellipse" | "diamond" | "text" | "arrow" | "line"，请合理安排 x, y 坐标避免重叠。`,
      );
    }
    extras.push(
      "你配备了知识库 Agent 工具，可直接检索知识库（list_libraries / get_library_tree / search_library / read_document）、查看历史与标签/书签（list_recent_history / list_marks_and_tags / get_node_tags）、管理标签与书签（set_node_tags / add_bookmark），以及在可写库中新建或修改笔记/脑图/文件夹（create_note / edit_note / create_folder / rename_node / move_node / delete_node）。当用户要求查找、整理、总结或写入知识库时，请主动调用相应工具完成，并在完成后简洁汇报操作结果（包含目标库与文档名称）。",
    );
    return extras.length ? `${base}\n\n${extras.join("\n")}` : base;
  }

  const isDocInRefs = (d) =>
    !!d && refs.some((r) => r.lib === d.lib && (r.path || "") === (d.path || "") && (r.bookId || "") === (d.bookId || ""));

  async function gatherContextBlocks() {
    const blocks = [];
    // 1. Current active document context (skip if already pinned in `refs` to avoid duplication)
    if (includeDoc && doc && !isDocInRefs(doc)) {
      let text = "";
      try {
        text = (await doc.getText?.()) || "";
      } catch {}
      if (!text && doc.lib && (doc.path || doc.bookId)) {
        try {
          const d = await api.aiDocText(doc.lib, doc.bookId || null, doc.path || null);
          text = d.text || "";
        } catch {}
      }
      if (text.trim()) {
        const clipped = text.length > 20000 ? text.slice(0, 20000) + "\n…（正文过长已截断）" : text;
        blocks.push(`【当前${docKindLabel}：《${doc.title || doc.path}》】\n${clipped}`);
      }
    }

    // 2. Current active search results context
    if (includeSearch && searchContext?.hits?.length) {
      const lines = searchContext.hits.slice(0, 20).map((h, idx) => {
        const snips = (h.snippets || [])
          .map((s) => `  - [${s.lb}] ${(s.parts || []).map((p) => p.t).join("")}`)
          .join("\n");
        const context = [h.library_title, h.book_id, h.subtitle].filter(Boolean).join(" · ");
        return `${idx + 1}. 《${h.title}》${context ? ` (${context})` : ""}\n${snips}`;
      });
      blocks.push(
        `【当前全文检索结果（关键词：「${searchContext.query || "全部"}」，共 ${searchContext.totalBooks == null ? `${searchContext.totalVolumes} 篇` : `${searchContext.totalBooks} 部、${searchContext.totalVolumes} 卷`}，以下为当前页命中摘录）】\n${lines.join("\n")}`,
      );
    }

    // 3. Explicitly referenced documents (`refs`)
    for (const r of refs) {
      const cacheKey = `${r.lib}|${r.bookId || ""}|${r.path || ""}`;
      let fetched = docCache.get(cacheKey);
      if (!fetched) {
        try {
          fetched = await api.aiDocText(r.lib, r.bookId || null, r.path || null);
          docCache.set(cacheKey, fetched);
        } catch (e) {
          blocks.push(`【引用文档《${r.title}》读取失败：${e}】`);
          continue;
        }
      }
      if (fetched?.text) {
        const clipped = fetched.text.length > 18000 ? fetched.text.slice(0, 18000) + "\n…（已截断）" : fetched.text;
        blocks.push(`【引用知识库文档：《${fetched.title}》（${fetched.lib_title}）】\n${clipped}`);
      }
    }

    // 4. Selected passage quotes (with sources)
    if (quotes.length === 1) {
      const q = quotes[0];
      const src = q.source ? `（来源：${q.source}）` : "";
      blocks.push(`【用户选中的文本片段${src}】\n${q.text}`);
    } else if (quotes.length > 1) {
      const parts = quotes.map((q, i) => {
        const src = q.source ? `（来源：${q.source}）` : "";
        return `片段 ${i + 1}${src}：\n${q.text}`;
      });
      blocks.push(`【用户选中的文本片段（共 ${quotes.length} 段）】\n\n${parts.join("\n\n")}`);
    }
    return blocks.join("\n\n---\n\n");
  }

  let reqCounter = 0;
  let unlistenStream = null;
  let cachedTools = null;
  let abortedRun = false;
  let pendingStream = null; // { reqId, resolve, reject }

  async function ensureTools() {
    if (!cachedTools) {
      cachedTools = await api.aiToolDefinitions().catch(() => []);
    }
    return cachedTools?.length ? cachedTools : null;
  }

  const TOOL_LABELS = {
    list_libraries: "列出知识库",
    get_library_tree: "浏览目录树",
    search_library: "搜索知识库",
    quick_search: "标题快搜",
    read_document: "读取文档",
    list_recent_history: "查看最近历史",
    list_marks_and_tags: "查看书签与标签",
    get_node_tags: "读取文档标签",
    dict_lookup: "查询词典",
    dict_search: "搜索词条",
    set_node_tags: "设置文档标签",
    add_bookmark: "添加书签/摘录",
    create_note: "新建文档",
    edit_note: "编辑文档",
    create_folder: "新建文件夹",
    rename_node: "重命名文档",
    move_node: "移动文档",
    delete_node: "删除文档",
  };

  function formatToolCall(tc) {
    const label = TOOL_LABELS[tc.name] || tc.name;
    try {
      const args = JSON.parse(tc.arguments || "{}");
      const detail = args.query || args.word || args.title || args.name || args.path || args.id || args.lib || "";
      return detail ? `${label}: ${String(detail).slice(0, 36)}` : label;
    } catch {
      return label;
    }
  }

  onMount(async () => {
    unlistenStream = await api.onAiStream((ev) => {
      if (!activeReqId || ev.req_id !== activeReqId) return;
      const last = messages[messages.length - 1];
      if (last && last.role === "assistant") {
        if (ev.reasoning) last.reasoning = (last.reasoning || "") + ev.reasoning;
        if (ev.delta) last.content += ev.delta;
        scrollBottom();
      }
      if (ev.done) {
        const waiter = pendingStream;
        pendingStream = null;
        activeReqId = null;
        if (waiter && waiter.reqId === ev.req_id) {
          if (ev.error) waiter.reject(new Error(ev.error));
          else waiter.resolve(ev.tool_calls || []);
        }
      }
    });
  });

  onDestroy(() => {
    abortedRun = true;
    if (activeReqId) api.aiAbort(activeReqId).catch(() => {});
    unlistenStream?.();
  });

  function stopStream() {
    abortedRun = true;
    if (activeReqId) {
      api.aiAbort(activeReqId).catch(() => {});
      activeReqId = null;
    }
    if (pendingStream) {
      const waiter = pendingStream;
      pendingStream = null;
      waiter.resolve([]);
    }
    busy = false;
    persistSession();
  }

  function streamOnce(apiMessages, tools) {
    return new Promise((resolve, reject) => {
      const reqId = `chat-${Date.now()}-${++reqCounter}`;
      activeReqId = reqId;
      pendingStream = { reqId, resolve, reject };
      api
        .aiStream(
          reqId,
          toBackendAiConfig(curPreset),
          buildSystemPrompt(),
          apiMessages,
          reasoningEffort || null,
          tools,
        )
        .catch((e) => {
          if (pendingStream?.reqId === reqId) {
            pendingStream = null;
            activeReqId = null;
            reject(e);
          }
        });
    });
  }

  async function sendPrompt(customText = null) {
    const text = (customText ?? input).trim();
    if (!text || busy) return;
    if (!curPreset?.baseUrl?.trim() || !curPreset?.model?.trim()) {
      errorMsg = `请先在「设置（${formatShortcut("settings", settings.shortcuts) || formatCombo("Ctrl+,")}）› AI 助手」中配置模型接口地址与模型名称。`;
      return;
    }
    errorMsg = "";
    statusMsg = "";
    abortedRun = false;
    if (customText == null) input = "";

    const contextBlock = await gatherContextBlocks();
    const displayContent = quotes.length
      ? `${quotes
          .map((q) => {
            const prefix = q.source ? `**[${q.source}]**\n` : "";
            return `> ${(prefix + q.text).replace(/\n/g, "\n> ")}`;
          })
          .join("\n\n")}\n\n${text}`
      : text;
    quotes = [];

    // Prepare history for API call (keep last 16 turns + context on the latest user message)
    const apiMessages = messages
      .filter((m) => m.content && m.content.trim())
      .slice(-16)
      .map((m) => ({ role: m.role, content: m.content }));

    const apiUserContent = contextBlock ? `${contextBlock}\n\n---\n\n用户指令：${text}` : text;
    apiMessages.push({ role: "user", content: apiUserContent });

    const isFirstExchange = messages.length === 0 && !customTitled;
    messages.push({ role: "user", content: displayContent, ts: Date.now() });
    messages.push({ role: "assistant", content: "", reasoning: "", toolLogs: [], ts: Date.now() });
    scrollBottom();

    busy = true;
    const tools = await ensureTools();
    const MAX_ROUNDS = 10;

    try {
      for (let round = 0; round < MAX_ROUNDS; round++) {
        if (abortedRun) break;
        const toolCalls = await streamOnce(apiMessages, tools);
        if (abortedRun || !toolCalls.length) break;

        const last = messages[messages.length - 1];
        apiMessages.push({
          role: "assistant",
          content: last.content || "",
          tool_calls: toolCalls,
        });

        for (const tc of toolCalls) {
          if (abortedRun) break;
          const logItem = {
            id: tc.id,
            name: tc.name,
            status: "running",
            summary: formatToolCall(tc),
          };
          last.toolLogs = [...(last.toolLogs || []), logItem];
          scrollBottom();

          let res;
          try {
            res = await api.aiExecTool(tc.name, tc.arguments || "{}");
          } catch (e) {
            res = { ok: false, output: String(e), mutated: false };
          }

          last.toolLogs = last.toolLogs.map((it) =>
            it.id === tc.id ? { ...it, status: res.ok ? "ok" : "err" } : it,
          );
          if (res.mutated) {
            onLibraryChanged?.(res.affected_lib, res.affected_path);
          }
          apiMessages.push({
            role: "tool",
            tool_call_id: tc.id,
            content: res.output || "",
          });
        }
        if (last.content && !last.content.endsWith("\n")) {
          last.content += "\n\n";
        }
      }
    } catch (e) {
      errorMsg = String(e);
    } finally {
      busy = false;
      activeReqId = null;
      pendingStream = null;
      const u0 = messages[0]?.content || "";
      const a0 = messages[1]?.content || "";
      await persistSession();
      if (isFirstExchange && sessionId && a0.trim()) {
        const targetId = sessionId;
        api
          .aiSummarizeTitle(toBackendAiConfig(curPreset), u0, a0)
          .then(async (shortTitle) => {
            if (!shortTitle || customTitled || sessionId !== targetId) return;
            sessionTitle = shortTitle;
            customTitled = true;
            await api.aiSessionRename(targetId, shortTitle).catch(() => {});
            onSessionChange?.({ sessionId: targetId, title: shortTitle });
          })
          .catch(() => {});
      }
    }
  }

  // ---- Actions on assistant replies (copy, apply to editor/mindmap/whiteboard, save as new note)
  async function copyReply(text) {
    try {
      await navigator.clipboard.writeText(text);
      statusMsg = "已复制到剪贴板";
    } catch (e) {
      errorMsg = String(e);
    }
  }

  async function applyToCurrentDoc(mode, text) {
    if (!onEditDoc) return;
    try {
      await onEditDoc({ mode, text });
      statusMsg = mode === "replace" ? `已应用并更新当前${docKindLabel}` : `已追加到当前${docKindLabel}`;
    } catch (e) {
      errorMsg = String(e);
    }
  }

  let saveModal = $state(null); // { text, kind, initialTitle, generatingTitle }

  function fallbackNoteTitle(text) {
    const firstLine = text
      .split("\n")
      .map((l) => l.replace(/^[#*>\-\s]+/, "").trim())
      .find((l) => l.length >= 2);
    return (firstLine || (scopeKind === "search" ? `创新方案-${searchContext?.query || "检索"}` : "AI 笔记"))
      .replace(/[\\/:*?"<>|]+/g, "_")
      .slice(0, 16);
  }

  async function generateShortNoteTitle(userText, replyText) {
    try {
      const generated = await api.aiSummarizeTitle(
        toBackendAiConfig(curPreset),
        userText || "请为以下内容拟定简短笔记标题",
        replyText.slice(0, 800),
      );
      const clean = (generated || "").replace(/[\\/:*?"<>|]+/g, "_").trim().slice(0, 20);
      if (clean) return clean;
    } catch {}
    return fallbackNoteTitle(replyText);
  }

  async function createAndSaveNote({ lib, libKind, path, title, text, kind, rememberDefault = false }) {
    if (rememberDefault) {
      updateSettings({
        aiSaveLib: lib || "",
        aiSavePath: path || "",
        aiSaveMode: "default",
      });
    }
    const r =
      libKind === "folder" && lib
        ? await api.createFolderFile(lib, path || "", kind === "mindmap" ? "mindmap" : "md", title)
        : await api.createNoteAt(lib || null, path || "", title, kind === "mindmap" ? "mindmap" : null);
    const content = kind === "mindmap" ? await api.aiMindmapParse(text, title) : text.trim() + "\n";
    await api.saveNote(r.lib, r.id, content);
    emit("data-changed").catch(() => {});
    emit("ai-note-created", { lib: r.lib, id: r.id }).catch(() => {});
    onLibraryChanged?.(r.lib, r.id);
    statusMsg = `已创建新${kind === "mindmap" ? "脑图" : "笔记"}：《${title}》`;
    if (onOpenDoc) onOpenDoc({ lib: r.lib, path: r.id });
  }

  async function saveAsNewNote(text, kind = "md", msgIdx = -1) {
    const prevUser = msgIdx > 0 && messages[msgIdx - 1]?.role === "user" ? messages[msgIdx - 1].content : "";
    if (settings.aiSaveMode === "default") {
      try {
        statusMsg = "正在由 AI 生成标题并保存…";
        const title = await generateShortNoteTitle(prevUser, text);
        let libKind = "notes";
        if (settings.aiSaveLib) {
          const st = await api.status().catch(() => null);
          const found = st?.libraries?.find((l) => l.id === settings.aiSaveLib);
          if (found) libKind = found.kind;
        }
        await createAndSaveNote({
          lib: settings.aiSaveLib || null,
          libKind,
          path: settings.aiSavePath || "",
          title,
          text,
          kind,
        });
      } catch (e) {
        errorMsg = String(e);
      }
      return;
    }

    const fallback = fallbackNoteTitle(text);
    const modalObj = { text, kind, initialTitle: fallback, generatingTitle: true };
    saveModal = modalObj;
    generateShortNoteTitle(prevUser, text).then((aiTitle) => {
      if (saveModal === modalObj) {
        saveModal = { ...modalObj, initialTitle: aiTitle || fallback, generatingTitle: false };
      }
    });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<aside class="ai-panel" data-scope-drop onscopedrop={onScopeDrop}>
  <header class="ai-head">
    <span class="brand"><Icon name="sparkles" size={16} /> <strong>AI 助手</strong></span>
    <select
      class="preset-sel"
      aria-label="选择大模型预设"
      value={curPreset.id}
      onchange={(e) => selectPreset(e.currentTarget.value)}
      title="随时切换大模型预设">
      {#each settings.aiPresets || [] as p (p.id)}
        <option value={p.id}>{p.name}</option>
      {/each}
    </select>
    <span class="grow"></span>
    <button class="icon-btn" onclick={newSession} title="新建对话">＋新对话</button>
    <button class="icon-btn" class:on={showHistory} onclick={() => (showHistory = true)} title="查看、搜索或恢复会话历史">历史</button>
    {#if onPopout}
      <button class="icon-btn" onclick={onPopout} title="在独立窗口中打开">新窗口</button>
    {/if}
    {#if onClose}
      <button class="icon-btn" onclick={onClose} title="收起 AI 助手" aria-label="收起">×</button>
    {/if}
  </header>

  {#if showHistory}
    <AiHistoryDialog
      activeSessionId={sessionId}
      {doc}
      onselect={selectSession}
      onrename={(id, title) => {
        if (id === sessionId) {
          sessionTitle = title;
          customTitled = true;
          onSessionChange?.({ sessionId: id, title });
        }
      }}
      onclearcurrent={newSession}
      onclose={() => (showHistory = false)} />
  {/if}

  <div class="ctx-bar">
    {#if doc}
      <label class="ctx-check" title="将当前激活标签的文档内容作为上下文发给大模型">
        <input type="checkbox" bind:checked={includeDoc} />
        <span>当前{docKindLabel}：《{doc.title || doc.path}》</span>
      </label>
      {#if !isDocInRefs(doc)}
        <button
          type="button"
          class="link pin-btn"
          onclick={() => addRefItem({ lib: doc.lib, book_id: doc.bookId, path: doc.path, title: doc.title || doc.path })}
          title="将当前文档固定到引用列表（切换标签页后仍保留在会话上下文中）">
          +固定引用
        </button>
      {/if}
    {:else if searchContext}
      <label class="ctx-check" title="将当前搜索词与命中结果摘录发给大模型">
        <input type="checkbox" bind:checked={includeSearch} />
        <span>当前搜索结果（{searchContext.hits?.length || 0} 篇命中摘录）</span>
      </label>
    {/if}
    <button type="button" class="ref-btn" onclick={() => openPicker("")} title="搜索并引用一个或多个知识库中的文档（也可输入 @ 或从左侧目录树拖入节点）">
      ＋引用文档
    </button>
  </div>

  {#if refs.length}
    <div class="refs-bar" title="已引用的知识库文档（可从左侧目录树继续拖入）">
      {#each refs as r, i (`${r.lib}|${r.bookId || ""}|${r.path}`)}
        <span class="ref-chip">
          <button
            type="button"
            class="link ref-link"
            onclick={() => onOpenDoc?.({ lib: r.lib, bookId: r.bookId, path: r.path })}
            title="点击打开该文档">
            📚 {r.title}
          </button>
          <button type="button" class="link" onclick={() => removeRef(i)} aria-label="移除引用">×</button>
        </span>
      {/each}
    </div>
  {/if}

  {#if showPicker}
    <div class="picker-pop">
      <div class="picker-head">
        <input
          type="search"
          bind:this={pickerInputEl}
          bind:value={pickerQuery}
          oninput={onPickerInput}
          onkeydown={(e) => e.key === "Escape" && (showPicker = false)}
          placeholder="输入经名、文章或笔记标题搜索引用…" />
        <button class="link" onclick={() => (showPicker = false)}>关闭</button>
      </div>
      <ul class="picker-list">
        {#if pickerBusy}
          <li class="muted empty">搜索中…</li>
        {:else}
          {#each pickerResults as it}
            <li>
              <button type="button" onclick={() => addRefItem(it)}>
                <span class="p-title">{it.title}</span>
                <span class="p-sub muted">{[it.lib_title, it.book_id, it.author].filter(Boolean).join(" · ")}</span>
              </button>
            </li>
          {:else}
            <li class="muted empty">没有匹配的文档（也可直接从左侧目录树拖动节点到此处）</li>
          {/each}
        {/if}
      </ul>
    </div>
  {/if}

  <div class="msg-list" bind:this={listEl}>
    {#if !messages.length}
      {#if !aiConfigured}
        <div class="setup-card">
          <div class="setup-title"><Icon name="sparkles" size={16} /> <strong>1 分钟启用 AI 助手 & 知识库 Agent</strong></div>
          <p class="muted setup-desc">配置任意大模型接口后，即可在阅读、搜索与写作旁随时对话，并让内置 Agent 自动检索知识库、引用多篇文档、生成笔记 / 脑图 / 白板。</p>
          <div class="setup-tpls" role="group" aria-label="常用服务商模板">
            {#each AI_PROVIDER_TEMPLATES as tpl (tpl.id)}
              <button type="button" class="tpl-chip" class:on={setupTpl === tpl.id} onclick={() => applySetupTemplate(tpl)}>
                {tpl.label}
              </button>
            {/each}
          </div>
          <div class="setup-fields">
            <label>
              <span class="muted">接口地址</span>
              <input type="text" bind:value={setupBaseUrl} placeholder="https://api.deepseek.com/v1" />
            </label>
            <label>
              <span class="muted">API Key {isLocalAiUrl(setupBaseUrl) ? "（本地模型可留空）" : ""}</span>
              <div class="key-row">
                <input type={setupShowKey ? "text" : "password"} bind:value={setupApiKey} placeholder={isLocalAiUrl(setupBaseUrl) ? "可留空" : "sk-..."} />
                <button type="button" class="link" onclick={() => (setupShowKey = !setupShowKey)}>{setupShowKey ? "隐藏" : "显示"}</button>
              </div>
            </label>
            <label>
              <span class="muted">模型名称</span>
              <input type="text" bind:value={setupModel} placeholder="deepseek-chat / gpt-4o / qwen2.5" />
            </label>
          </div>
          {#if setupMsg}
            <p class="setup-msg" class:err={setupErr}>{setupMsg}</p>
          {/if}
          <div class="setup-actions">
            <button type="button" class="primary" onclick={saveQuickSetup} disabled={setupTesting}>保存并开始</button>
            <button type="button" onclick={testAndSaveQuickSetup} disabled={setupTesting}>{setupTesting ? "测试中…" : "测试并保存"}</button>
            {#if onOpenSettings}
              <button type="button" class="link" onclick={() => onOpenSettings("ai")}>更多设置…</button>
            {/if}
          </div>
        </div>
      {:else}
      <div class="welcome">
        <p class="muted">
          {#if searchContext}
            可根据当前全文搜索结果（及引用的多篇知识库文档）综合归纳或编写创新方案：
          {:else if doc?.contentType === "mindmap"}
            描述主题或大纲要求，AI 可直接生成或扩展当前脑图节点：
          {:else if doc?.contentType === "whiteboard"}
            描述流程图、概念关系或架构，AI 可直接在当前白板上绘制图形：
          {:else if doc}
            可结合当前{docKindLabel}、切换标签页或从目录树拖入多篇文档进行协同对话：
          {:else}
            内置知识库 Agent 可自动搜索与整理你的资料，也可输入 <code>@</code> 或拖入目录节点引用文档：
          {/if}
        </p>
        <div class="quick-prompts">
          {#if searchContext}
            <button onclick={() => sendPrompt("请基于当前搜索命中的文献结果与摘录，提炼核心观点，并编写一份结构完整、有理有据的创新方案（包含背景洞察、核心思想要点、创新结合点与具体实施步骤）。")}>
              💡 根据搜索结果编写创新方案
            </button>
            <button onclick={() => sendPrompt("请综合归纳当前搜索命中结果的核心义理与脉络异同，并列出每处观点的文献依据。")}>
              📊 综合提炼搜索结果要点
            </button>
            <button onclick={() => sendPrompt("请根据当前搜索结果，梳理出一份结构化的专题研究大纲（使用缩进列表格式，便于转为脑图或笔记）。")}>
              🧭 生成专题研究大纲
            </button>
          {:else if doc?.contentType === "mindmap"}
            <button onclick={() => sendPrompt("请根据当前脑图主题（及引用文档），生成一份层次丰富的完整脑图大纲（使用 Markdown 缩进列表格式）。")}>
              🧠 生成完整脑图分支
            </button>
            <button onclick={() => sendPrompt("请对当前脑图的各个分支进行深化扩充，补充关键子集与核心要点（使用 Markdown 缩进列表格式）。")}>
              ✨ 扩充深化现有脑图
            </button>
          {:else if doc?.contentType === "whiteboard"}
            <button onclick={() => sendPrompt("请根据当前主题（或引用文档内容），设计并绘制一张核心概念与流程关系白板图（输出 excalidraw 代码块）。")}>
              🎨 绘制核心概念关系图
            </button>
            <button onclick={() => sendPrompt("请梳理当前白板中的文字要素，重新规划布局并绘制一张清晰的架构/演进流程图（输出 excalidraw 代码块）。")}>
              📐 绘制结构化流程图
            </button>
          {:else if doc}
            <button onclick={() => sendPrompt("请总结本篇文档的核心脉络与主要观点。")}>
              📝 总结本篇核心脉络
            </button>
            <button onclick={() => sendPrompt("请解析本篇文档中的关键概念、名相或难点，并用通俗清晰的现代语言加以说明。")}>
              🔍 解析关键概念与难点
            </button>
            <button onclick={() => sendPrompt("请结合本篇文档内容，启发式地提出几个可深入研究或创新应用的思路方案。")}>
              💡 构思延伸研究与创新方案
            </button>
          {:else}
            <button onclick={() => sendPrompt("请帮我盘点当前已添加的知识库列表与大致目录结构，并建议我可以怎么高效整理或检索。")}>
              🤖 Agent：盘点我的知识库结构
            </button>
            <button onclick={() => openPicker("")}>
              📚 引用指定知识库文档开始对话…
            </button>
          {/if}
        </div>
      </div>
      {/if}
    {:else}
      {#each messages as m, idx}
        <div class="msg {m.role}">
          {#if m.reasoning}
            <details class="reasoning">
              <summary class="muted">思考过程</summary>
              <pre>{m.reasoning}</pre>
            </details>
          {/if}
          {#if m.toolLogs?.length}
            <div class="tool-logs">
              {#each m.toolLogs as t (t.id)}
                <span class="tool-pill" class:ok={t.status === "ok"} class:err={t.status === "err"}>
                  {t.status === "running" ? "⏳" : t.status === "ok" ? "✓" : "✕"} {t.summary}
                </span>
              {/each}
            </div>
          {/if}
          <div class="msg-body">{@html renderMd(m.content || (busy && idx === messages.length - 1 ? "思考中…" : ""))}</div>
          {#if m.role === "assistant" && m.content}
            <div class="msg-actions">
              <button class="link" onclick={() => copyReply(m.content)}>复制</button>
              {#if onEditDoc && doc?.writable}
                {#if doc.contentType === "mindmap"}
                  <button class="link accent" onclick={() => applyToCurrentDoc("replace", m.content)}>应用为整张脑图</button>
                  <button class="link accent" onclick={() => applyToCurrentDoc("append", m.content)}>追加到脑图节点</button>
                {:else if doc.contentType === "whiteboard"}
                  <button class="link accent" onclick={() => applyToCurrentDoc("append", m.content)}>绘制到白板（追加）</button>
                  <button class="link accent" onclick={() => applyToCurrentDoc("replace", m.content)}>替换白板画布</button>
                {:else}
                  <button class="link accent" onclick={() => applyToCurrentDoc("append", m.content)}>追加到当前笔记</button>
                  <button class="link" onclick={() => applyToCurrentDoc("replace", m.content)}>替换当前笔记</button>
                {/if}
              {/if}
              <button class="link" onclick={() => saveAsNewNote(m.content, "md", idx)}>新笔记</button>
              <button class="link" onclick={() => saveAsNewNote(m.content, "mindmap", idx)}>新脑图</button>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  {#if quotes.length}
    <div class="quote-box">
      <div class="quote-head">
        <span class="muted">已选片段（{quotes.length}）</span>
        <span class="quote-quick">
          <button class="link" onclick={() => sendPrompt("请详细解释以上片段的含义与背景。")}>解释</button>
          <button class="link" onclick={() => sendPrompt("请将以上片段翻译成通俗晓畅的现代白话文。")}>白话翻译</button>
          <button class="link" onclick={() => sendPrompt("请根据以上片段提炼要点并构思创新应用方案。")}>编写方案</button>
          <button class="link" onclick={() => (quotes = [])} title="清空全部片段">清空</button>
        </span>
      </div>
      <div class="quote-list">
        {#each quotes as q, i (i)}
          <div class="quote-item">
            <div class="quote-item-body">
              {#if q.source}<span class="quote-src">{q.source}</span>{/if}
              <span class="quote-text">{q.text}</span>
            </div>
            <button type="button" class="link quote-del" onclick={() => (quotes = quotes.filter((_, idx) => idx !== i))} title="移除此片段" aria-label="移除此片段">×</button>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if errorMsg}
    <div class="ai-err">{errorMsg}</div>
  {/if}
  {#if statusMsg}
    <div class="ai-status muted">{statusMsg}</div>
  {/if}

  <form class="composer" onsubmit={(e) => { e.preventDefault(); sendPrompt(); }}>
    <textarea
      bind:this={inputEl}
      bind:value={input}
      onkeydown={onInputKey}
      rows="3"
      placeholder="输入问题或指令（Enter 发送，Shift+Enter 换行，输入 @ 引用知识库文档）…"></textarea>
    <div class="composer-foot">
      {#if modelChoices.length > 0}
        <select
          class="effort-sel model-sel"
          aria-label="切换当前模型"
          value={curPreset.model}
          onchange={(e) => selectModel(e.currentTarget.value)}
          title="在当前预设已缓存的模型间切换">
          {#each modelChoices as m (m)}
            <option value={m}>{m}</option>
          {/each}
        </select>
      {/if}
      <select
        class="effort-sel"
        aria-label="推理深度"
        value={reasoningEffort}
        onchange={(e) => setEffort(e.currentTarget.value)}
        title="调整大模型推理深度（仅支持推理的模型生效；选「默认」时不额外传参）">
        {#each EFFORTS as [val, label]}
          <option value={val}>{label}</option>
        {/each}
      </select>
      <span class="grow"></span>
      {#if busy}
        <button type="button" class="stop-btn" onclick={stopStream}>停止生成</button>
      {:else}
        <button type="submit" class="primary" disabled={!input.trim()}>发送</button>
      {/if}
    </div>
  </form>
  {#if saveModal}
    <AiSaveNoteDialog
      kind={saveModal.kind}
      initialTitle={saveModal.initialTitle}
      generatingTitle={saveModal.generatingTitle}
      onConfirm={async ({ lib, libKind, path, title, rememberDefault }) => {
        await createAndSaveNote({
          lib,
          libKind,
          path,
          title,
          text: saveModal.text,
          kind: saveModal.kind,
          rememberDefault,
        });
      }}
      onClose={() => (saveModal = null)} />
  {/if}
</aside>

<style>
  .ai-panel {
    display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 260px;
    background: var(--panel); border-left: 1px solid var(--line); font-size: 14px;
  }
  .ai-panel:global(.scope-over) { outline: 2px dashed var(--accent); outline-offset: -3px; }
  .ai-head {
    display: flex; align-items: center; gap: 6px; padding: 6px 10px;
    border-bottom: 1px solid var(--line); background: var(--paper); flex-wrap: wrap;
  }
  .brand { display: inline-flex; align-items: center; gap: 4px; color: var(--accent); white-space: nowrap; }
  .preset-sel { max-width: 155px; font-size: 12px; padding: 2px 6px; }
  .grow { flex: 1; }
  .icon-btn { font-size: 12px; padding: 2px 7px; border-radius: 5px; }
  .icon-btn.on { background: var(--accent); color: var(--paper); border-color: var(--accent); }

  .ctx-bar {
    display: flex; align-items: center; justify-content: space-between; gap: 6px;
    padding: 5px 10px; border-bottom: 1px solid var(--line); font-size: 12px; flex-wrap: wrap;
  }
  .ctx-check { display: flex; align-items: center; gap: 4px; min-width: 0; flex: 1; }
  .ctx-check span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pin-btn { font-size: 11px; flex: none; padding: 1px 4px; }
  .ref-btn { font-size: 12px; padding: 1px 8px; flex: none; }

  .refs-bar {
    display: flex; flex-wrap: wrap; gap: 4px; padding: 5px 10px;
    border-bottom: 1px solid var(--line); background: var(--bg);
  }
  .ref-chip {
    display: inline-flex; align-items: center; gap: 4px; font-size: 12px;
    background: var(--accent-soft); border-radius: 10px; padding: 1px 6px 1px 8px; max-width: 100%;
  }
  .ref-link { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 180px; }

  .picker-pop {
    border-bottom: 1px solid var(--line); background: var(--paper); max-height: 240px;
    display: flex; flex-direction: column;
  }
  .picker-head {
    display: flex; align-items: center; gap: 6px; padding: 6px 10px; border-bottom: 1px solid var(--line); font-size: 12px;
  }
  .picker-head input { flex: 1; font-size: 13px; padding: 3px 8px; }
  .picker-list {
    list-style: none; margin: 0; padding: 4px 8px; overflow: auto; flex: 1;
  }
  .picker-list li button {
    width: 100%; text-align: left; border: none; background: none; padding: 4px 6px;
    display: flex; flex-direction: column; gap: 2px; border-radius: 4px;
  }
  .picker-list li button:hover { background: var(--accent-soft); }
  .p-title { font-weight: 600; font-size: 13px; }
  .p-sub { font-size: 11px; }
  .empty { padding: 8px 4px; font-size: 12px; }

  .msg-list { flex: 1; overflow: auto; padding: 10px 12px; display: flex; flex-direction: column; gap: 10px; }
  .welcome { padding: 8px 4px; }
  .setup-card {
    background: var(--paper); border: 1px solid var(--line); border-radius: 8px;
    padding: 12px; display: flex; flex-direction: column; gap: 10px;
  }
  .setup-title { display: flex; align-items: center; gap: 6px; color: var(--accent); font-size: 14px; }
  .setup-desc { margin: 0; font-size: 12.5px; line-height: 1.55; }
  .setup-tpls { display: flex; flex-wrap: wrap; gap: 5px; }
  .tpl-chip {
    font-size: 12px; padding: 3px 9px; border-radius: 999px; background: var(--bg);
    border: 1px solid var(--line); color: var(--ink);
  }
  .tpl-chip:hover { border-color: var(--accent); }
  .tpl-chip.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  .setup-fields { display: flex; flex-direction: column; gap: 7px; }
  .setup-fields label { display: flex; flex-direction: column; gap: 3px; font-size: 12px; }
  .setup-fields input { width: 100%; font-size: 13px; padding: 5px 8px; background: var(--bg); }
  .key-row { display: flex; align-items: center; gap: 6px; }
  .key-row input { flex: 1; min-width: 0; }
  .setup-msg { margin: 0; font-size: 12px; color: var(--note); }
  .setup-msg.err { color: var(--hl); }
  .setup-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding-top: 2px; }
  .setup-actions button { font-size: 13px; padding: 5px 12px; }
  .setup-actions .link { margin-left: auto; font-size: 12px; padding: 2px 4px; }
  .quick-prompts { display: flex; flex-direction: column; gap: 6px; margin-top: 8px; }
  .quick-prompts button { text-align: left; padding: 6px 10px; font-size: 13px; border-radius: 6px; background: var(--paper); }
  .quick-prompts button:hover { background: var(--accent-soft); border-color: var(--accent); }

  .msg {
    max-width: 88%; padding: 8px 11px; border-radius: 4px 12px 12px 12px;
    background: var(--paper); border: 1px solid var(--line); align-self: flex-start;
  }
  .msg.user {
    align-self: flex-end; background: var(--accent-soft); border-color: transparent;
    border-radius: 12px 4px 12px 12px;
  }
  .reasoning { margin-bottom: 6px; font-size: 12px; background: var(--bg); padding: 4px 8px; border-radius: 4px; }
  .reasoning pre { white-space: pre-wrap; margin: 4px 0 0; font-family: inherit; color: var(--muted); }
  .msg-body { line-height: 1.65; word-break: break-word; user-select: text; }
  .msg-body :global(p) { margin: 0.35em 0; }
  .msg-body :global(pre) { background: var(--bg); padding: 8px; border-radius: 6px; overflow-x: auto; font-size: 12px; }
  .msg-body :global(blockquote) { margin: 0.3em 0; padding-left: 10px; border-left: 3px solid var(--accent); color: var(--muted); }
  .msg-actions {
    display: flex; flex-wrap: wrap; gap: 8px; margin-top: 6px; padding-top: 5px;
    border-top: 1px dashed var(--line); font-size: 12px;
  }
  .msg-actions .accent { font-weight: 600; color: var(--accent); }

  .quote-box {
    margin: 0 10px 6px; padding: 6px 8px; background: var(--paper);
    border: 1px solid var(--accent); border-radius: 6px; font-size: 12px;
  }
  .quote-head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 4px; }
  .quote-quick { display: flex; gap: 6px; }
  .quote-list { max-height: 112px; overflow-y: auto; display: flex; flex-direction: column; gap: 4px; }
  .quote-item { display: flex; align-items: flex-start; gap: 6px; padding: 3px 4px; border-radius: 4px; background: var(--bg); }
  .quote-item-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .quote-src { font-size: 11px; color: var(--accent); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .quote-text { max-height: 44px; overflow: auto; color: var(--muted); white-space: pre-wrap; word-break: break-word; }
  .quote-del { flex: none; padding: 0 3px; line-height: 1.2; }

  .ai-err { margin: 0 10px 6px; padding: 6px 8px; border-radius: 6px; background: var(--hl-bg); color: var(--hl); font-size: 12px; }
  .ai-status { margin: 0 10px 4px; font-size: 12px; }

  .composer {
    display: flex; flex-direction: column; gap: 6px; padding: 8px 10px;
    border-top: 1px solid var(--line); background: var(--paper);
  }
  .composer textarea {
    width: 100%; box-sizing: border-box; resize: vertical; min-height: 54px; max-height: 180px;
    padding: 6px 8px; border: 1px solid var(--line); border-radius: 6px;
    background: var(--bg); color: inherit; font: inherit; line-height: 1.5;
  }
  .composer textarea:focus { outline: 1px solid var(--accent); border-color: var(--accent); }
  .composer-foot { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .effort-sel { font-size: 12px; padding: 2px 6px; max-width: 145px; }
  .model-sel { max-width: 150px; }
  .tool-logs { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 6px; }
  .tool-pill {
    font-size: 11px; padding: 2px 7px; border-radius: 999px;
    background: var(--bg); border: 1px solid var(--line); color: var(--muted);
    max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .tool-pill.ok { border-color: var(--accent); color: var(--accent); }
  .tool-pill.err { border-color: var(--hl); color: var(--hl); }
  .stop-btn { border-color: var(--hl); color: var(--hl); }
</style>
