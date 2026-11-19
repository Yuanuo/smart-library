// User settings: theme, fonts, sizes. Kept in the webview's local storage
// (which lives in the portable data folder) and applied as CSS variables, so
// every window (the main one and the pop-out readers) looks the same; a
// change in one window reaches the others through the `storage` event.
import { getCurrentWebview } from "@tauri-apps/api/webview";

// Font presets. `default` means "whatever the platform calls its own": the
// interface takes the system UI sans (Segoe UI / San Francisco / Noto Sans),
// reading text takes the system CJK serif (宋体 / 細明體 / Songti), which is
// what makes a page read like a book. The stacks themselves live in app.css as
// --font-ui and --font-serif; a null stack just drops the inline variable so
// those defaults apply. That is why the two lists have different defaults: the
// same word "default" lands on a sans for the UI and a serif for the reader.
const SANS = '"Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC", "Source Han Sans SC", "WenQuanYi Micro Hei", sans-serif';
const SERIF = '"Iowan Old Style", "Palatino Linotype", Palatino, Georgia, "Songti TC", "Songti SC", "Noto Serif TC", "Noto Serif CJK TC", "Source Han Serif TC", "PMingLiU", "MingLiU", "SimSun", serif';
const KAI = '"KaiTi", "Kaiti TC", "STKaiti", "AR PL UKai TW", serif';
const MONO = 'Consolas, "Courier New", "MingLiU", monospace';

export const UI_FONTS = [
  ["default", "系统默认（推荐）", null],
  ["serif", "宋体 / 明体", SERIF],
  ["kai", "楷体", KAI],
  ["mono", "等宽", MONO],
  ["custom", "自定义…", null],
];

export const READER_FONTS = [
  ["default", "宋体 / 明体（默认）", null],
  ["sans", "黑体 / 无衬线", SANS],
  ["kai", "楷体", KAI],
  ["mono", "等宽", MONO],
  ["custom", "自定义…", null],
];

// Reader text weight. CSS cannot scale a font's weight against a base (there
// is no rem for font-weight), and most CJK fonts only carry a couple of real
// faces, so this is a small table of levels rather than a slider. `stroke` is
// a fake-bold width in em: it thickens text even when the font has no face for
// the requested weight (0 = off). On fonts with only regular and bold the top
// level has no headroom left, so its emphasis adds a stroke of its own.
export const WEIGHT_LEVELS = [
  ["std", "标准"],
  ["mid", "稍粗"],
  ["bold", "加粗"],
];

// Select-to-look-up in the read-only reader. `ctrl` needs Ctrl/Cmd held while
// selecting (so ordinary selection and copy stay quiet); `auto` looks up every
// selection, and Ctrl/Cmd suppresses it (selecting under Ctrl usually means
// copying). `off` turns the feature off. Editors never use it: selecting text
// there is a constant part of editing.
export const SELECT_LOOKUP_MODES = [
  ["ctrl", "按住 Ctrl / Cmd 选字"],
  ["auto", "选中即查"],
  ["off", "关闭"],
];

const WEIGHTS = {
  std: { body: 400, strong: 700, stroke: 0, strongStroke: 0 },
  mid: { body: 500, strong: 800, stroke: 0.012, strongStroke: 0.012 },
  bold: { body: 650, strong: 900, stroke: 0, strongStroke: 0.024 },
};

const KEY = "sl.settings";
export const DEFAULT_AI_PRESET = {
  id: "default-openai",
  name: "默认模型（OpenAI 兼容）",
  format: "openai", // openai | anthropic
  baseUrl: "https://api.deepseek.com/v1",
  apiKey: "",
  model: "deepseek-chat",
  models: [],
  effortByModel: {},
  systemPrompt: "你是 Smart Library 知识库的 AI 助手，擅长经典文献研读、笔记撰写、创新方案构思、脑图大纲生成与白板绘图。",
};

const DEFAULTS = {
  theme: "auto", // auto | light | dark
  uiFont: "default",
  uiFontCustom: "",
  uiZoom: 100, // percent
  readerFont: "default",
  readerFontCustom: "",
  readerSize: 19, // px
  readerLine: 2, // line height
  readerSpacing: 0.04, // em
  readerPara: 0.35, // paragraph spacing, em
  readerWidth: 46, // max width of the reading column, em
  readerFullWidth: false, // stretch the reading column to the visible area
  readerWeight: "std", // std | mid | bold
  readerScript: "orig", // orig: as the text is | simp: shown in simplified characters
  selectLookup: "ctrl", // reader selection opens the dictionary: ctrl | auto | off
  externalLink: "window", // window | ask | browser
  quickLimit: 200, // title quick search: results per batch
  quickPinyin: false, // title quick search: allow matching by pinyin (full or initials)
  dailyOn: true, // the sentence of the day
  dailySources: [], // its sources (search range items); none = every library
  aiOn: false, // AI assistant master switch
  aiActiveId: "default-openai", // currently selected model preset id
  aiPresets: [{ ...DEFAULT_AI_PRESET }],
  aiSaveMode: "ask", // ask | default
  aiSaveLib: "", // default target library id
  aiSavePath: "", // default folder/parent path
  aiSnapshotMax: 10, // max temporary AI snapshots per document
  aiSnapshotDays: 7, // retention days for temporary AI snapshots
  shortcuts: {}, // { [actionId]: comboString } overrides
};

function load() {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) || "{}");
    const raw = { ...DEFAULTS, ...parsed };
    if (!raw.shortcuts || typeof raw.shortcuts !== "object" || Array.isArray(raw.shortcuts)) {
      raw.shortcuts = {};
    }
    if (!["window", "ask", "browser"].includes(raw.externalLink)) {
      raw.externalLink = "window";
    }
    if (!WEIGHTS[raw.readerWeight]) {
      raw.readerWeight = "std";
    }
    // "sans" used to be a UI font preset; the UI's default is that stack now
    if (raw.uiFont === "sans") {
      raw.uiFont = "default";
    }
    if (!SELECT_LOOKUP_MODES.some(([v]) => v === raw.selectLookup)) {
      raw.selectLookup = "ctrl";
    }
    if (!Array.isArray(raw.aiPresets) || !raw.aiPresets.length) {
      raw.aiPresets = [{ ...DEFAULT_AI_PRESET }];
    } else {
      raw.aiPresets = raw.aiPresets.map((p) => ({
        ...DEFAULT_AI_PRESET,
        ...p,
        models: Array.isArray(p?.models) ? p.models.filter((m) => typeof m === "string" && m.trim()) : [],
        effortByModel: p?.effortByModel && typeof p.effortByModel === "object" && !Array.isArray(p.effortByModel)
          ? p.effortByModel
          : {},
      }));
    }
    if (!raw.aiPresets.some((p) => p.id === raw.aiActiveId)) {
      raw.aiActiveId = raw.aiPresets[0].id;
    }
    return raw;
  } catch {
    return { ...DEFAULTS, aiPresets: [{ ...DEFAULT_AI_PRESET }] };
  }
}

export const settings = $state(load());

/** The currently selected AI model preset. */
export function activeAiPreset() {
  const list = settings.aiPresets ?? [];
  return list.find((p) => p.id === settings.aiActiveId) ?? list[0] ?? DEFAULT_AI_PRESET;
}

/** True when AI is enabled and the active preset has a URL and model name. */
export function isAiReady() {
  if (!settings.aiOn) return false;
  const p = activeAiPreset();
  return !!(p && p.baseUrl?.trim() && p.model?.trim());
}

/** Convert a preset to the backend `AiModelConfig` struct. */
export function toBackendAiConfig(preset = activeAiPreset()) {
  return {
    format: preset?.format || "openai",
    base_url: (preset?.baseUrl || "").trim(),
    api_key: (preset?.apiKey || "").trim(),
    model: (preset?.model || "").trim(),
  };
}

function stack(kind, custom, fallback, list) {
  if (kind === "custom") return custom.trim() ? `"${custom.trim().replace(/"/g, "")}", ${fallback}` : null;
  // a font installed on this computer: `sys:<family>`
  if (kind.startsWith("sys:")) return `"${kind.slice(4).replace(/"/g, "")}", ${fallback}`;
  // "default" (and any value from an older version) keeps the CSS default
  return list.find((f) => f[0] === kind)?.[2] ?? null;
}

let appliedZoom = 100;

/** Put the settings on the page. */
export function apply() {
  const root = document.documentElement;
  if (settings.theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", settings.theme);
  const set = (name, value) => (value == null ? root.style.removeProperty(name) : root.style.setProperty(name, value));
  set("--ui-font", stack(settings.uiFont, settings.uiFontCustom, "sans-serif", UI_FONTS));
  set("--reader-font", stack(settings.readerFont, settings.readerFontCustom, "serif", READER_FONTS));
  set("--reader-size", `${settings.readerSize}px`);
  set("--reader-line", String(settings.readerLine));
  set("--reader-spacing", `${settings.readerSpacing}em`);
  set("--reader-para", `${settings.readerPara}em`);
  set("--reader-max-width", settings.readerFullWidth ? "none" : `${settings.readerWidth}em`);
  const weight = WEIGHTS[settings.readerWeight] ?? WEIGHTS.std;
  set("--reader-weight", String(weight.body));
  set("--reader-strong-weight", String(weight.strong));
  set("--reader-stroke", `${weight.stroke}em`);
  set("--reader-strong-stroke", `${weight.strongStroke}em`);
  if (settings.uiZoom !== appliedZoom) {
    appliedZoom = settings.uiZoom;
    getCurrentWebview().setZoom(settings.uiZoom / 100).catch(() => {});
  }
}

export function update(patch) {
  Object.assign(settings, patch);
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...settings }));
  } catch {}
  apply();
}

export function reset() {
  update({ ...DEFAULTS });
}

export function initSettings() {
  apply();
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) {
      Object.assign(settings, load());
      apply();
    }
  });
}
