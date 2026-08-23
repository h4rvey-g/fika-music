import { describe, expect, it, vi } from "vitest";
import type { ShortcutEvent } from "@tauri-apps/plugin-global-shortcut";
import { GLOBAL_SHORTCUTS_STORAGE_KEY } from "../lib/global-shortcut-preferences";
import {
  useGlobalShortcuts,
  type GlobalShortcutDependencies,
} from "./use-global-shortcuts";

function setup(storedPreferences: Record<string, string | null> = {}) {
  const handlers = new Map<string, (event: ShortcutEvent) => void>();
  const register = vi.fn(async (shortcut: string, handler: (event: ShortcutEvent) => void) => {
    handlers.set(shortcut, handler);
  });
  const unregister = vi.fn(async (shortcut: string) => {
    handlers.delete(shortcut);
  });
  const dependencies: GlobalShortcutDependencies = {
    isTauri: () => true,
    register,
    unregister,
  };
  const storage = {
    getItem: vi.fn().mockReturnValue(JSON.stringify(storedPreferences)),
    setItem: vi.fn(),
  };
  const handler = vi.fn();
  const shortcuts = useGlobalShortcuts(handler, dependencies, storage);
  return { handler, handlers, register, shortcuts, storage, unregister };
}

function shortcutEvent(shortcut: string, state: "Pressed" | "Released"): ShortcutEvent {
  return { id: 1, shortcut, state };
}

describe("system-wide shortcut registration", () => {
  it("registers persisted bindings and dispatches only pressed events", async () => {
    const shortcut = "CommandOrControl+Shift+KeyP";
    const context = setup({ togglePlayback: shortcut });
    await context.shortcuts.initialize();

    context.handlers.get(shortcut)?.(shortcutEvent(shortcut, "Released"));
    expect(context.handler).not.toHaveBeenCalled();
    context.handlers.get(shortcut)?.(shortcutEvent(shortcut, "Pressed"));
    expect(context.handler).toHaveBeenCalledWith("togglePlayback");
  });

  it("registers a new binding before persisting it", async () => {
    const context = setup();
    const shortcut = "CommandOrControl+Shift+KeyM";

    expect(await context.shortcuts.setBinding("toggleMute", shortcut)).toBe(true);
    expect(context.register).toHaveBeenCalledWith(shortcut, expect.any(Function));
    expect(context.shortcuts.bindings.value.toggleMute).toBe(shortcut);
    expect(context.storage.setItem).toHaveBeenCalledWith(
      GLOBAL_SHORTCUTS_STORAGE_KEY,
      expect.stringContaining(shortcut),
    );
  });

  it("rejects a duplicate binding without calling the system API", async () => {
    const shortcut = "CommandOrControl+Shift+KeyP";
    const context = setup({ togglePlayback: shortcut });

    expect(await context.shortcuts.setBinding("nextTrack", shortcut)).toBe(false);
    expect(context.register).not.toHaveBeenCalled();
    expect(context.shortcuts.errors.value.nextTrack).toEqual({
      code: "duplicate",
      conflictingAction: "togglePlayback",
    });
  });

  it("keeps the previous binding when the replacement is unavailable", async () => {
    const previous = "CommandOrControl+Shift+KeyP";
    const replacement = "CommandOrControl+Alt+KeyP";
    const context = setup({ togglePlayback: previous });
    await context.shortcuts.initialize();
    context.register.mockRejectedValueOnce(new Error("shortcut already taken"));

    expect(await context.shortcuts.setBinding("togglePlayback", replacement)).toBe(false);
    expect(context.shortcuts.bindings.value.togglePlayback).toBe(previous);
    expect(context.unregister).not.toHaveBeenCalled();
    expect(context.shortcuts.errors.value.togglePlayback).toEqual({ code: "unavailable" });
  });

  it("rolls back a replacement when the previous binding cannot be removed", async () => {
    const previous = "CommandOrControl+Shift+KeyP";
    const replacement = "CommandOrControl+Alt+KeyP";
    const context = setup({ togglePlayback: previous });
    await context.shortcuts.initialize();
    context.unregister.mockRejectedValueOnce(new Error("unregister failed"));

    expect(await context.shortcuts.setBinding("togglePlayback", replacement)).toBe(false);
    expect(context.unregister).toHaveBeenLastCalledWith(replacement);
    expect(context.shortcuts.bindings.value.togglePlayback).toBe(previous);
    expect(context.shortcuts.errors.value.togglePlayback).toEqual({ code: "unregister" });
  });

  it("clears and disposes only bindings owned by this manager", async () => {
    const playback = "CommandOrControl+Shift+KeyP";
    const mute = "CommandOrControl+Shift+KeyM";
    const context = setup({ togglePlayback: playback, toggleMute: mute });
    await context.shortcuts.initialize();

    expect(await context.shortcuts.clearBinding("toggleMute")).toBe(true);
    await context.shortcuts.dispose();
    expect(context.unregister).toHaveBeenCalledWith(mute);
    expect(context.unregister).toHaveBeenCalledWith(playback);
    expect(context.unregister).toHaveBeenCalledTimes(2);
  });
});
