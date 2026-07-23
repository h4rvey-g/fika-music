import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_UI_PREFERENCES,
  THEME_OPTIONS,
  UI_PREFERENCES_STORAGE_KEY,
  loadUiPreferences,
  parseUiPreferences,
  saveUiPreferences,
} from "./ui-preferences";

describe("UI preferences", () => {
  it("falls back to defaults when persisted preferences are malformed", () => {
    const storage = {
      getItem: vi.fn().mockReturnValue("not-json"),
    };

    expect(loadUiPreferences(storage)).toEqual(DEFAULT_UI_PREFERENCES);
  });

  it("validates enum values and clamps volume", () => {
    expect(
      parseUiPreferences({
        theme: "sepia",
        density: "compact",
        streamQuality: "lossless-plus",
        audioSourceId: "invalid source!",
        volume: 4,
        playbackMode: "repeat-one",
      }),
    ).toEqual({
      theme: "system",
      density: "compact",
      streamQuality: "128k",
      audioSourceId: "",
      volume: 1,
      playbackMode: "sequential",
    });
  });

  it("accepts every available theme", () => {
    for (const theme of THEME_OPTIONS) {
      expect(parseUiPreferences({ theme: theme.value }).theme).toBe(theme.value);
    }
  });

  it("migrates the legacy audio source preference to a standalone source id", () => {
    expect(
      parseUiPreferences({
        audioSourceFamily: "imported-lx-source",
      }).audioSourceId,
    ).toBe("imported-lx-source");
  });

  it("writes normalized preferences to the application key", () => {
    const storage = {
      setItem: vi.fn(),
    };

    saveUiPreferences(
      {
        theme: "dark",
        density: "comfortable",
        streamQuality: "flac",
        audioSourceId: "imported-lx-source",
        volume: -1,
        playbackMode: "shuffle",
      },
      storage,
    );

    expect(storage.setItem).toHaveBeenCalledWith(
      UI_PREFERENCES_STORAGE_KEY,
      JSON.stringify({
        theme: "dark",
        density: "comfortable",
        streamQuality: "flac",
        audioSourceId: "imported-lx-source",
        volume: 0,
        playbackMode: "shuffle",
      }),
    );
  });
});
