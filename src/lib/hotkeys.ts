export interface HotkeyKeyboardInput {
  code: string;
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const MODIFIER_CODES = new Set([
  "MetaLeft",
  "MetaRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
]);

const SPECIAL_KEYS: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  NumpadEnter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  ArrowUp: "ArrowUp",
  ArrowDown: "ArrowDown",
  ArrowLeft: "ArrowLeft",
  ArrowRight: "ArrowRight",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Insert: "Insert",
  Minus: "Minus",
  Equal: "Equal",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Quote: "Quote",
  Comma: "Comma",
  Period: "Period",
  Slash: "Slash",
  Backquote: "Backquote",
};

export function keyCodeToAccelerator(code: string): string | null {
  if (code.startsWith("Key") && code.length == 4) return code.slice(3);
  if (code.startsWith("Digit") && code.length == 6) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (code.startsWith("Numpad") && /^\d$/.test(code.slice(-1))) return code.slice(-1);
  return SPECIAL_KEYS[code] || null;
}

export function buildHotkeyFromEvent(event: HotkeyKeyboardInput): string | null {
  if (MODIFIER_CODES.has(event.code)) return null;
  const key = keyCodeToAccelerator(event.code);
  if (!key) return null;
  const modifiers: string[] = [];
  if (event.metaKey) modifiers.push("Super");
  if (event.ctrlKey) modifiers.push("Control");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (!modifiers.length && !/^F\d+$/.test(key)) return null;
  return [...modifiers, key].join("+");
}

export function formatHotkey(value: string, isMac: boolean): string {
  if (!value) return "未设置";
  const parts = value.split("+");
  if (!isMac) {
    return parts
      .map((part) => (part == "Super" ? "Win" : part == "Control" ? "Ctrl" : part))
      .join("+");
  }
  const key = parts[parts.length - 1] || "";
  const modifierOrder = ["Control", "Alt", "Shift", "Super"];
  const modifiers = parts
    .slice(0, -1)
    .sort((left, right) => modifierOrder.indexOf(left) - modifierOrder.indexOf(right));
  return [...modifiers, key]
    .map((part) => {
      if (part == "Super" || part == "Command") return "⌘";
      if (part == "Control") return "⌃";
      if (part == "Alt" || part == "Option") return "⌥";
      if (part == "Shift") return "⇧";
      return part;
    })
    .join("");
}

export function isModifierOnly(event: HotkeyKeyboardInput): boolean {
  return MODIFIER_CODES.has(event.code);
}
