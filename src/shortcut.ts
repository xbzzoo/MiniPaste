/**
 * 快捷键的录制与显示。
 *
 * 存储格式与 Tauri global-shortcut 一致，如 `Shift+Cmd+V`、`Alt+Space`、`Ctrl+ArrowLeft`；
 * 显示规则与 Rust `menu::pretty_shortcut` 一致：修饰键按 macOS 习惯排成 ⌃⌥⇧⌘。
 */

const MODIFIERS = [
  { name: "Ctrl", symbol: "⌃", aliases: ["ctrl", "control"] },
  { name: "Alt", symbol: "⌥", aliases: ["alt", "option"] },
  { name: "Shift", symbol: "⇧", aliases: ["shift"] },
  {
    name: "Cmd",
    symbol: "⌘",
    aliases: ["cmd", "command", "super", "cmdorctrl", "commandorcontrol"],
  },
] as const;

const KEY_SYMBOLS: Record<string, string> = {
  arrowup: "↑",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
  enter: "↩",
  backspace: "⌫",
  delete: "⌦",
  escape: "⎋",
  tab: "⇥",
  pageup: "⇞",
  pagedown: "⇟",
  home: "↖",
  end: "↘",
  backquote: "`",
  minus: "-",
  equal: "=",
  bracketleft: "[",
  bracketright: "]",
  backslash: "\\",
  semicolon: ";",
  quote: "'",
  comma: ",",
  period: ".",
  slash: "/",
};

const isModifier = (token: string) =>
  MODIFIERS.some((m) => (m.aliases as readonly string[]).includes(token));

/** `Shift+Cmd+V` → `["⇧", "⌘", "V"]`，每一项画成一个键帽 */
export function shortcutParts(accelerator: string): string[] {
  const tokens = accelerator
    .split("+")
    .map((t) => t.trim().toLowerCase())
    .filter(Boolean);
  const mods = MODIFIERS.filter((m) =>
    tokens.some((t) => (m.aliases as readonly string[]).includes(t)),
  ).map((m) => m.symbol);
  const keys = tokens.filter((t) => !isModifier(t)).map(keyLabel);
  return [...mods, ...keys];
}

export const formatShortcut = (accelerator: string) =>
  shortcutParts(accelerator).join("");

function keyLabel(token: string): string {
  const bare = token.replace(/^(key|digit)/, "");
  return KEY_SYMBOLS[bare] ?? bare.charAt(0).toUpperCase() + bare.slice(1);
}

/** 修饰键自身的 keydown，不构成完整组合 */
const MODIFIER_CODE = /^(Shift|Control|Alt|Meta|OS|CapsLock|Fn)/;

export interface Recorded {
  /** 当前按住的修饰键符号，还没按主键时用来实时显示 */
  held: string[];
  /** 完整组合（含主键）；只按了修饰键时为 null */
  accelerator: string | null;
}

/** 从键盘事件录制。用 `event.code`（物理键位），按住 ⌥ 时 `event.key` 会变成特殊字符 */
export function recordKey(event: KeyboardEvent): Recorded {
  const flags = [event.ctrlKey, event.altKey, event.shiftKey, event.metaKey];
  const active = MODIFIERS.filter((_, i) => flags[i]);
  const held = active.map((m) => m.symbol);

  if (!event.code || MODIFIER_CODE.test(event.code)) {
    return { held, accelerator: null };
  }
  const key = event.code.replace(/^Key/, "").replace(/^Digit/, "");
  return {
    held,
    accelerator: [...active.map((m) => m.name), key].join("+"),
  };
}
