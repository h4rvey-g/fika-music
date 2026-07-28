import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_STATE_EVENT,
  DESKTOP_LYRICS_WINDOW_LABEL,
  type DesktopLyricsState,
} from "./desktop-lyrics";
import { syncDesktopLyricsWindow, syncMenuBarLyrics } from "./desktop-lyrics-window";

const tauriMocks = vi.hoisted(() => ({
  emitTo: vi.fn().mockResolvedValue(undefined),
  getByLabel: vi.fn(),
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriMocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ emitTo: tauriMocks.emitTo }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: { getByLabel: tauriMocks.getByLabel },
}));

const nativeWindow = {
  setMinSize: vi.fn().mockResolvedValue(undefined),
  setAlwaysOnTop: vi.fn().mockResolvedValue(undefined),
  setIgnoreCursorEvents: vi.fn().mockResolvedValue(undefined),
  show: vi.fn().mockResolvedValue(undefined),
  hide: vi.fn().mockResolvedValue(undefined),
};

function state(enabled: boolean): DesktopLyricsState {
  return {
    title: "Track",
    subtitle: "Artist",
    currentLine: "Current line",
    currentLineKey: "timed:0:0",
    currentLineStartMs: 0,
    currentLineEndMs: 2_000,
    currentWords: [],
    currentTimingSource: "estimated",
    nextLine: "Next line",
    isPlaying: true,
    clockRunning: true,
    playbackRate: 1,
    playbackPositionMs: 500,
    preferences: { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES, enabled },
  };
}

describe("desktop lyrics window coordination", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getByLabel.mockResolvedValue(nativeWindow);
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  afterEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("configures, shows, and broadcasts to an enabled lyrics window", async () => {
    const payload = state(true);

    await syncDesktopLyricsWindow(payload);

    expect(tauriMocks.getByLabel).toHaveBeenCalledWith(DESKTOP_LYRICS_WINDOW_LABEL);
    expect(nativeWindow.setMinSize).toHaveBeenCalledOnce();
    expect(nativeWindow.setAlwaysOnTop).toHaveBeenCalledWith(true);
    expect(nativeWindow.setIgnoreCursorEvents).toHaveBeenCalledWith(false);
    expect(nativeWindow.show).toHaveBeenCalledOnce();
    expect(nativeWindow.hide).not.toHaveBeenCalled();
    expect(tauriMocks.emitTo).toHaveBeenCalledWith(
      DESKTOP_LYRICS_WINDOW_LABEL,
      DESKTOP_LYRICS_STATE_EVENT,
      payload,
    );
  });

  it("hides a disabled lyrics window without broadcasting stale state", async () => {
    await syncDesktopLyricsWindow(state(false));

    expect(nativeWindow.hide).toHaveBeenCalledOnce();
    expect(nativeWindow.show).not.toHaveBeenCalled();
    expect(tauriMocks.emitTo).not.toHaveBeenCalled();
  });

  it("syncs an active lyric line to the native menu bar item", async () => {
    const payload = state(false);
    payload.preferences.menuBarEnabled = true;

    await syncMenuBarLyrics(payload);

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_menu_bar_lyrics", {
      enabled: true,
      line: "Current line",
      title: "Track",
      subtitle: "Artist",
      maxWidth: 40,
    });
  });

  it("keeps the menu bar item enabled for playback status messages", async () => {
    const payload = state(false);
    payload.preferences.menuBarEnabled = true;
    payload.currentLineKey = "message:Lyrics unavailable";

    await syncMenuBarLyrics(payload);

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_menu_bar_lyrics", {
      enabled: true,
      line: "",
      title: "Track",
      subtitle: "Artist",
      maxWidth: 40,
    });
  });

  it("disables the menu bar item when the preference is off", async () => {
    const payload = state(false);

    await syncMenuBarLyrics(payload);

    expect(tauriMocks.invoke).toHaveBeenCalledWith(
      "set_menu_bar_lyrics",
      expect.objectContaining({ enabled: false }),
    );
  });
});
