import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
  NOW_PLAYING_LYRICS_STORAGE_KEY,
  NOW_PLAYING_LYRICS_THEME_COLOR,
  loadNowPlayingLyricsPreferences,
  parseNowPlayingLyricsPreferences,
  saveNowPlayingLyricsPreferences,
} from "./now-playing-lyrics";

describe("now playing lyrics preferences", () => {
  it("falls back to defaults when persisted preferences are malformed", () => {
    const storage = {
      getItem: vi.fn().mockReturnValue("not-json"),
    };

    expect(loadNowPlayingLyricsPreferences(storage)).toEqual(
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
    );
  });

  it("validates style choices and clamps numeric values", () => {
    expect(
      parseNowPlayingLyricsPreferences({
        font: "comic",
        fontSize: 99,
        lineGap: -5,
        activeFontWeight: 900,
        alignment: "justify",
        activeColor: "not-a-color",
        inactiveColor: "#123456",
        inactiveOpacity: 0,
      }),
    ).toEqual({
      ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
      fontSize: 30,
      lineGap: 4,
      inactiveColor: "#123456",
      inactiveOpacity: 0.1,
    });
  });

  it("writes normalized preferences to the application key", () => {
    const storage = {
      setItem: vi.fn(),
    };

    saveNowPlayingLyricsPreferences(
      {
        ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
        font: "rounded",
        fontSize: 24,
        alignment: "left",
        activeColor: "#22cc88",
        inactiveColor: NOW_PLAYING_LYRICS_THEME_COLOR,
      },
      storage,
    );

    expect(storage.setItem).toHaveBeenCalledWith(
      NOW_PLAYING_LYRICS_STORAGE_KEY,
      JSON.stringify({
        ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
        font: "rounded",
        fontSize: 24,
        alignment: "left",
        activeColor: "#22cc88",
        inactiveColor: NOW_PLAYING_LYRICS_THEME_COLOR,
      }),
    );
  });
});
