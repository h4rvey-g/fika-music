import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
  GLOBAL_SHORTCUTS_STORAGE_KEY,
  captureGlobalShortcut,
  globalShortcutAriaKeys,
  globalShortcutDisplayKeys,
  loadGlobalShortcutPreferences,
  parseGlobalShortcutPreferences,
  saveGlobalShortcutPreferences,
} from "./global-shortcut-preferences";

function keyboardEvent(code: string, init: KeyboardEventInit = {}) {
  return new KeyboardEvent("keydown", { code, ...init });
}

describe("global shortcut preferences", () => {
  it("defaults every action to disabled when storage is malformed", () => {
    const storage = { getItem: vi.fn().mockReturnValue("not-json") };

    expect(loadGlobalShortcutPreferences(storage)).toEqual(DEFAULT_GLOBAL_SHORTCUT_PREFERENCES);
  });

  it("keeps valid unique bindings and rejects malformed or duplicate values", () => {
    expect(parseGlobalShortcutPreferences({
      togglePlayback: "CommandOrControl+Shift+KeyP",
      previousTrack: "CommandOrControl+Shift+KeyP",
      nextTrack: "KeyN",
      volumeUp: "Alt+ArrowUp",
    })).toEqual({
      ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
      togglePlayback: "CommandOrControl+Shift+KeyP",
      volumeUp: "Alt+ArrowUp",
    });
  });

  it("captures physical keys with portable platform modifiers", () => {
    expect(captureGlobalShortcut(
      keyboardEvent("KeyP", { metaKey: true, shiftKey: true }),
      "mac",
    )).toEqual({ shortcut: "CommandOrControl+Shift+KeyP", error: null });
    expect(captureGlobalShortcut(
      keyboardEvent("ArrowRight", { ctrlKey: true, altKey: true }),
      "other",
    )).toEqual({ shortcut: "CommandOrControl+Alt+ArrowRight", error: null });
  });

  it("rejects modifier-only, bare, and unsupported keys", () => {
    expect(captureGlobalShortcut(keyboardEvent("ShiftLeft", { shiftKey: true }), "other").error)
      .toBe("modifier-only");
    expect(captureGlobalShortcut(keyboardEvent("Space"), "other").error)
      .toBe("modifier-required");
    expect(captureGlobalShortcut(keyboardEvent("KeyM", { shiftKey: true }), "other").error)
      .toBe("modifier-required");
    expect(captureGlobalShortcut(keyboardEvent("IntlYen", { ctrlKey: true }), "other").error)
      .toBe("unsupported-key");
  });

  it("formats visual and assistive-technology key names per platform", () => {
    const shortcut = "CommandOrControl+Shift+KeyP";
    expect(globalShortcutDisplayKeys(shortcut, "mac")).toEqual(["Cmd", "Shift", "P"]);
    expect(globalShortcutDisplayKeys(shortcut, "other")).toEqual(["Ctrl", "Shift", "P"]);
    expect(globalShortcutAriaKeys(shortcut, "mac")).toBe("Meta+Shift+P");
  });

  it("writes normalized preferences to the dedicated storage key", () => {
    const storage = { setItem: vi.fn() };
    saveGlobalShortcutPreferences({
      ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
      toggleMute: "CommandOrControl+Shift+KeyM",
    }, storage);

    expect(storage.setItem).toHaveBeenCalledWith(
      GLOBAL_SHORTCUTS_STORAGE_KEY,
      JSON.stringify({
        ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
        toggleMute: "CommandOrControl+Shift+KeyM",
      }),
    );
  });
});
