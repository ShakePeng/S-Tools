import { describe, expect, it } from "vitest";
import {
  buildHotkeyFromEvent,
  formatHotkey,
  isModifierOnly,
  keyCodeToAccelerator,
} from "./hotkeys";

describe("hotkey recorder", () => {
  it("maps keyboard codes to Tauri accelerator keys", () => {
    expect(keyCodeToAccelerator("KeyD")).toBe("D");
    expect(keyCodeToAccelerator("Digit1")).toBe("1");
    expect(keyCodeToAccelerator("F1")).toBe("F1");
    expect(keyCodeToAccelerator("Space")).toBe("Space");
  });

  it("captures modifiers in a stable order", () => {
    expect(
      buildHotkeyFromEvent({
        code: "KeyT",
        key: "t",
        metaKey: true,
        ctrlKey: true,
        altKey: true,
        shiftKey: true,
      }),
    ).toBe("Super+Control+Alt+Shift+T");
  });

  it("captures Control+Command+A as a Tauri accelerator", () => {
    expect(
      buildHotkeyFromEvent({
        code: "KeyA",
        key: "a",
        metaKey: true,
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe("Super+Control+A");
  });

  it("allows function keys without modifiers but rejects plain letters", () => {
    expect(
      buildHotkeyFromEvent({
        code: "F1",
        key: "F1",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe("F1");
    expect(
      buildHotkeyFromEvent({
        code: "KeyA",
        key: "a",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBeNull();
  });

  it("formats Mac and Windows labels", () => {
    expect(formatHotkey("Super+Shift+T", true)).toBe("⇧⌘T");
    expect(formatHotkey("Super+Control+A", true)).toBe("⌃⌘A");
    expect(formatHotkey("Control+Alt+D", false)).toBe("Ctrl+Alt+D");
  });

  it("detects modifier-only key events", () => {
    expect(
      isModifierOnly({
        code: "AltRight",
        key: "Alt",
        metaKey: false,
        ctrlKey: false,
        altKey: true,
        shiftKey: false,
      }),
    ).toBe(true);
  });
});
