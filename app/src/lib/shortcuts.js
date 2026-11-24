// Keyboard shortcuts shared by the main window and the pop-out readers.
// User overrides are stored in `settings.shortcuts` (`{ [id]: comboString }`).
import { settings } from "./settings.svelte.js";
import { IS_MAC } from "./platform.js";

export const SHORTCUT_DEFS = [
  { id: "quick", label: "标题快搜", defaultKey: "Ctrl+G", note: "也可双击 Shift" },
  { id: "search", label: "全文搜索", defaultKey: "Ctrl+H" },
  { id: "dict", label: "查词典", defaultKey: "Ctrl+K", note: "有选中文字时查这个词" },
  { id: "recent", label: "最近访问", defaultKey: "Ctrl+E" },
  { id: "marks", label: "收藏与标签", defaultKey: "Ctrl+B" },
  { id: "settings", label: "打开设置", defaultKey: "Ctrl+," },
  { id: "diary", label: "今日日记", defaultKey: "Ctrl+J" },
  { id: "ai", label: "AI 助手", defaultKey: "F1" },
  { id: "tag", label: "当前文档标签", defaultKey: "Ctrl+T", docOnly: true },
  { id: "bookmark", label: "当前文档书签", defaultKey: "Ctrl+D", docOnly: true },
  { id: "props", label: "当前文档属性", defaultKey: "Ctrl+P", docOnly: true },
  { id: "locate", label: "在目录树中定位", defaultKey: "Ctrl+L", docOnly: true },
  { id: "new", label: "新建笔记", defaultKey: "Ctrl+N", docOnly: true },
  { id: "new-mindmap", label: "新建脑图", defaultKey: "Ctrl+Shift+2" },
  { id: "new-whiteboard", label: "新建白板", defaultKey: "Ctrl+Shift+3" },
  { id: "close", label: "关闭当前标签", defaultKey: "Ctrl+W", docOnly: true },
  { id: "next-tab", label: "下一个标签", defaultKey: "Ctrl+Tab" },
  { id: "prev-tab", label: "上一个标签", defaultKey: "Ctrl+Shift+Tab" },
  { id: "nav-back", label: "上一个视图（后退）", defaultKey: "Ctrl+Shift+ArrowLeft" },
  { id: "nav-forward", label: "下一个视图（前进）", defaultKey: "Ctrl+Shift+ArrowRight" },
  { id: "script", label: "简体 / 原文切换", defaultKey: "Ctrl+Shift+S" },
  { id: "rename", label: "重命名（目录树）", defaultKey: "F2", local: true },
  { id: "save", label: "保存（笔记 / 脑图 / 白板）", defaultKey: "Ctrl+S", local: true },
];

const DEF_MAP = new Map(SHORTCUT_DEFS.map((d) => [d.id, d]));

const CODE_KEY_MAP = {
  Space: "Space",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
};

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "CapsLock", "NumLock", "ScrollLock"]);

const DISPLAY_KEY_MAP = {
  Ctrl: IS_MAC ? "Cmd" : "Ctrl",
  Alt: IS_MAC ? "Option" : "Alt",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

let recording = false;

/** Pause global/local shortcut handling while recording a new shortcut in Settings. */
export function setShortcutRecording(active) {
  recording = !!active;
}

/** Return the active combo string for `id` (may be `""` if cleared). */
export function getShortcut(id, overrides = settings.shortcuts) {
  if (overrides && Object.prototype.hasOwnProperty.call(overrides, id)) {
    return overrides[id] ?? "";
  }
  return DEF_MAP.get(id)?.defaultKey ?? "";
}

/** Split a combo string into displayable keycap labels (platform-aware). */
export function comboParts(combo) {
  if (!combo) return [];
  return combo.split("+").filter(Boolean).map((k) => DISPLAY_KEY_MAP[k] ?? k);
}

/** Format a canonical combo string for display on the current platform (e.g. `"Cmd+G"` on macOS, `"Ctrl+G"` on Windows/Linux). */
export function formatCombo(combo) {
  return comboParts(combo).join("+");
}

/** Format the active shortcut for `id` on the current platform (may be `""` if cleared). */
export function formatShortcut(id, overrides = settings.shortcuts) {
  return formatCombo(getShortcut(id, overrides));
}

function normalizeEventKey(e) {
  if (!e.key || MODIFIER_KEYS.has(e.key)) return null;
  const code = e.code || "";
  if (code.startsWith("Digit") && code.length === 6) return code.slice(5);
  if (code.startsWith("Key") && code.length === 4) return code.slice(3).toUpperCase();
  if (CODE_KEY_MAP[code]) return CODE_KEY_MAP[code];
  if (e.key === " ") return "Space";
  if (e.key.length === 1) return e.key.toUpperCase();
  return e.key;
}

/** Convert a KeyboardEvent to a canonical combo string (e.g. `"Ctrl+Shift+S"`), or `null` if modifier-only. */
export function eventToCombo(e) {
  const key = normalizeEventKey(e);
  if (!key) return null;
  const parts = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

function parseCombo(combo) {
  if (!combo) return null;
  const parts = combo.split("+").filter(Boolean);
  if (!parts.length) return null;
  let ctrl = false;
  let alt = false;
  let shift = false;
  let key = "";
  for (const p of parts) {
    if (p === "Ctrl") ctrl = true;
    else if (p === "Alt") alt = true;
    else if (p === "Shift") shift = true;
    else key = p;
  }
  return key ? { ctrl, alt, shift, key: key.toLowerCase() } : null;
}

/** True when KeyboardEvent `e` matches the given combo string. */
export function matchesCombo(e, combo) {
  const parsed = parseCombo(combo);
  if (!parsed) return false;
  if ((e.ctrlKey || e.metaKey) !== parsed.ctrl) return false;
  if (!!e.altKey !== parsed.alt) return false;
  if (!!e.shiftKey !== parsed.shift) return false;
  const key = normalizeEventKey(e);
  return !!key && key.toLowerCase() === parsed.key;
}

/** True when KeyboardEvent `e` matches the configured shortcut for `id`. */
export function matchesShortcut(e, id) {
  if (recording) return false;
  return matchesCombo(e, getShortcut(id));
}

/** The text selected in a reader (not in an input), else "". */
export function readerSelection() {
  const s = window.getSelection();
  if (!s || s.isCollapsed || !s.rangeCount) return "";
  const n = s.anchorNode;
  const el = n?.nodeType === 1 ? n : n?.parentElement;
  if (!el?.closest?.(".reader")) return "";
  return s.toString().trim();
}

export function installShortcuts(fire) {
  let lastShift = 0;
  let shiftAlone = false;
  // about the document: not where a drawing board or a mind map has its own use of the keys
  const ownKeys = ".excalidraw, .mind-elixir, .map-container";
  const globalDefs = SHORTCUT_DEFS.filter((d) => !d.local);

  function down(e) {
    if (recording) return;

    for (const def of globalDefs) {
      const combo = getShortcut(def.id);
      if (!combo || !matchesCombo(e, combo)) continue;
      if (def.docOnly && e.target?.closest?.(ownKeys)) continue;
      e.preventDefault();
      shiftAlone = false;
      fire(def.id);
      return;
    }

    // Legacy secondary shortcut: Ctrl+Shift+1 -> new note when `new` is not overridden.
    const overrides = settings.shortcuts || {};
    if (!Object.prototype.hasOwnProperty.call(overrides, "new") && matchesCombo(e, "Ctrl+Shift+1")) {
      e.preventDefault();
      shiftAlone = false;
      fire("new");
      return;
    }

    if (e.key === "Shift") {
      if (!e.repeat) shiftAlone = true;
    } else {
      shiftAlone = false;
      lastShift = 0;
    }
  }

  // Shift pressed twice (no other key in between, within 400 ms)
  function up(e) {
    if (recording) return;
    if (e.key !== "Shift" || !shiftAlone) return;
    const now = performance.now();
    if (now - lastShift < 400) {
      lastShift = 0;
      fire("quick");
    } else {
      lastShift = now;
    }
  }

  window.addEventListener("keydown", down, true);
  window.addEventListener("keyup", up, true);
  return () => {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
  };
}
