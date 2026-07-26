import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_STORAGE_KEY,
  desktopLyricsMinimumHeight,
  desktopLyricsOutlineColor,
  loadDesktopLyricsPreferences,
  parseDesktopLyricsPreferences,
  resolveDesktopLyricLines,
  saveDesktopLyricsPreferences,
} from "./desktop-lyrics";

describe("desktop lyrics", () => {
  it("chooses a contrasting text-effect color for custom lyric colors", () => {
    expect(desktopLyricsOutlineColor("#f8fafc")).toBe("rgb(0 0 0 / 82%)");
    expect(desktopLyricsOutlineColor("#111827")).toBe("rgb(255 255 255 / 88%)");
  });

  it("falls back to defaults when persisted preferences are malformed", () => {
    expect(loadDesktopLyricsPreferences({ getItem: vi.fn().mockReturnValue("not-json") }))
      .toEqual(DEFAULT_DESKTOP_LYRICS_PREFERENCES);
  });

  it("validates colors and enums while clamping numeric style values", () => {
    expect(parseDesktopLyricsPreferences({
      enabled: true,
      menuBarEnabled: true,
      menuBarMaxWidth: 100,
      locked: "yes",
      activeColor: "skyblue",
      inactiveColor: "#abcDEF",
      backgroundOpacity: 4,
      fontSize: 8,
      fontWeight: 900,
      font: "comic-sans",
      alignment: "justify",
      effect: "glow",
    })).toEqual({
      ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
      enabled: true,
      menuBarEnabled: true,
      menuBarMaxWidth: 56,
      inactiveColor: "#abcDEF",
      backgroundOpacity: 1,
      fontSize: 18,
    });
  });

  it("writes normalized preferences to the desktop lyrics key", () => {
    const storage = { setItem: vi.fn() };
    saveDesktopLyricsPreferences({
      ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
      enabled: true,
      fontSize: 200,
    }, storage);

    expect(storage.setItem).toHaveBeenCalledWith(
      DESKTOP_LYRICS_STORAGE_KEY,
      JSON.stringify({
        ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
        enabled: true,
        fontSize: 72,
      }),
    );
  });

  it("selects the current and next synchronized lyric lines", () => {
    expect(resolveDesktopLyricLines({
      source: "embedded",
      provider: null,
      isSynced: true,
      savedPath: null,
      matchScore: null,
      lines: [
        { startMs: 1_000, endMs: 3_000, text: "First line", words: [] },
        { startMs: 3_000, endMs: 6_000, text: "Second line", words: [] },
        { startMs: 6_000, endMs: null, text: "Third line", words: [] },
      ],
    }, 3.5)).toEqual(expect.objectContaining({
      currentLine: "Second line",
      currentLineStartMs: 3_000,
      currentLineEndMs: 6_000,
      currentTimingSource: "estimated",
      nextLine: "Third line",
    }));
  });

  it("shows the first lines for unsynchronized lyrics", () => {
    expect(resolveDesktopLyricLines({
      source: "sidecar",
      provider: null,
      isSynced: false,
      savedPath: null,
      matchScore: null,
      lines: [
        { startMs: null, endMs: null, text: "Plain one", words: [] },
        { startMs: null, endMs: null, text: "Plain two", words: [] },
      ],
    }, 42)).toEqual(expect.objectContaining({
      currentLine: "Plain one",
      currentTimingSource: null,
      nextLine: "Plain two",
    }));
  });

  it("keeps source word timing while expanding it to grapheme progress", () => {
    const result = resolveDesktopLyricLines({
      source: "network",
      provider: "NetEase",
      isSynced: true,
      savedPath: null,
      matchScore: 100,
      lines: [
        {
          startMs: 1_000,
          endMs: 2_000,
          text: "Hi!",
          words: [
            { startMs: 1_000, endMs: 1_800, text: "Hi" },
            { startMs: 1_800, endMs: 2_000, text: "!" },
          ],
        },
      ],
    }, 1.2, 3);

    expect(result.currentTimingSource).toBe("source");
    expect(result.currentWords.map((word) => word.text).join("")).toBe("Hi!");
    expect(result.currentWords[0]).toEqual({ text: "H", startMs: 1_000, endMs: 1_400 });
    expect(result.currentWords[result.currentWords.length - 1]?.endMs).toBe(2_000);
  });

  it("keeps an untimed translation inactive after source-timed lyrics", () => {
    const result = resolveDesktopLyricLines({
      source: "network",
      provider: "NetEase",
      isSynced: true,
      savedPath: null,
      matchScore: 100,
      lines: [
        {
          startMs: 1_000,
          endMs: 2_000,
          text: "Original\nTranslation",
          words: [{ startMs: 1_000, endMs: 2_000, text: "Original" }],
        },
      ],
    }, 1.5, 3);

    expect(result.currentTimingSource).toBe("source");
    expect(result.currentWords.map((word) => word.text).join(""))
      .toBe("Original\nTranslation");
    expect(result.currentWords[result.currentWords.length - 1]).toEqual({
      text: "\nTranslation",
      startMs: 2_000,
      endMs: 2_000,
      isTimed: false,
    });
  });

  it("derives character timings from adjacent line timestamps", () => {
    const result = resolveDesktopLyricLines({
      source: "sidecar",
      provider: null,
      isSynced: true,
      savedPath: null,
      matchScore: null,
      lines: [
        { startMs: 2_000, endMs: 4_000, text: "AB", words: [] },
        { startMs: 4_000, endMs: null, text: "Next", words: [] },
      ],
    }, 2.5);

    expect(result.currentWords).toEqual([
      { text: "A", startMs: 2_000, endMs: 3_000 },
      { text: "B", startMs: 3_000, endMs: 4_000 },
    ]);
  });

  it("derives line and character timing for unsynchronized lyrics from track duration", () => {
    const result = resolveDesktopLyricLines({
      source: "sidecar",
      provider: null,
      isSynced: false,
      savedPath: null,
      matchScore: null,
      lines: [
        { startMs: null, endMs: null, text: "AB", words: [] },
        { startMs: null, endMs: null, text: "CD", words: [] },
      ],
    }, 7, 10);

    expect(result).toEqual(expect.objectContaining({
      currentLine: "CD",
      currentLineStartMs: 5_000,
      currentLineEndMs: 10_000,
      currentTimingSource: "estimated",
    }));
    expect(result.currentWords[0]).toEqual({ text: "C", startMs: 5_000, endMs: 7_500 });
  });

  it("grows the minimum window height with the configured lyric size", () => {
    expect(desktopLyricsMinimumHeight({
      ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
      fontSize: 72,
    })).toBeGreaterThan(
      desktopLyricsMinimumHeight(DEFAULT_DESKTOP_LYRICS_PREFERENCES),
    );
  });
});
