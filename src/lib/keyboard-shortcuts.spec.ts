import { describe, expect, it } from "vitest";
import {
  APP_SHORTCUTS,
  isInteractiveShortcutTarget,
  matchKeyboardShortcut,
  shortcutAriaKeys,
  shortcutDisplayBindings,
} from "./keyboard-shortcuts";

function keyboardEvent(key: string, init: KeyboardEventInit = {}) {
  return new KeyboardEvent("keydown", { key, ...init });
}

describe("keyboard shortcuts", () => {
  it("matches unmodified playback keys without accepting extra modifiers", () => {
    expect(matchKeyboardShortcut(keyboardEvent(" "), "other")?.shortcut.id)
      .toBe("togglePlayback");
    expect(matchKeyboardShortcut(keyboardEvent("M"), "other")?.shortcut.id)
      .toBe("toggleMute");
    expect(matchKeyboardShortcut(keyboardEvent("ArrowRight"), "other")?.shortcut.id)
      .toBe("seekForward");
    expect(matchKeyboardShortcut(keyboardEvent("ArrowRight", { altKey: true }), "other"))
      .toBeNull();
  });

  it("uses the native platform modifier for application commands and track navigation", () => {
    expect(matchKeyboardShortcut(keyboardEvent("k", { ctrlKey: true }), "other")?.shortcut.id)
      .toBe("focusSearch");
    expect(matchKeyboardShortcut(keyboardEvent("ArrowLeft", { ctrlKey: true }), "other")?.shortcut.id)
      .toBe("previousTrack");
    expect(matchKeyboardShortcut(keyboardEvent(",", { metaKey: true }), "mac")?.shortcut.id)
      .toBe("openSettings");
    expect(matchKeyboardShortcut(keyboardEvent("k", { ctrlKey: true }), "mac"))
      .toBeNull();
  });

  it("supports standard media keys without showing them in the shortcut sheet", () => {
    const match = matchKeyboardShortcut(keyboardEvent("MediaPlayPause"), "other");

    expect(match?.shortcut.id).toBe("togglePlayback");
    expect(match?.binding.allowInInteractive).toBe(true);
    expect(shortcutDisplayBindings("togglePlayback", "other")).toEqual([["Space"]]);
    expect(shortcutAriaKeys("togglePlayback", "other"))
      .toBe("Space MediaPlayPause");
  });

  it("formats visible and assistive-technology key names per platform", () => {
    expect(shortcutDisplayBindings("focusSearch", "other")).toEqual([["Ctrl", "K"]]);
    expect(shortcutDisplayBindings("focusSearch", "mac")).toEqual([["Cmd", "K"]]);
    expect(shortcutAriaKeys(["seekBackward", "seekForward"], "mac"))
      .toBe("ArrowLeft ArrowRight");
  });

  it("recognizes controls and editable surfaces that own their keyboard input", () => {
    const input = document.createElement("input");
    const buttonChild = document.createElement("span");
    const button = document.createElement("button");
    const grid = document.createElement("div");
    const paragraph = document.createElement("p");
    button.append(buttonChild);
    grid.setAttribute("role", "grid");

    expect(isInteractiveShortcutTarget(input)).toBe(true);
    expect(isInteractiveShortcutTarget(buttonChild)).toBe(true);
    expect(isInteractiveShortcutTarget(grid)).toBe(true);
    expect(isInteractiveShortcutTarget(paragraph)).toBe(false);
  });

  it("keeps shortcut identifiers unique", () => {
    expect(new Set(APP_SHORTCUTS.map((shortcut) => shortcut.id)).size)
      .toBe(APP_SHORTCUTS.length);
  });
});
