import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_UI_PREFERENCES,
  THEME_GROUPS,
  THEME_MODE_OPTIONS,
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
        locale: "zh-TW",
        theme: "sepia",
        density: "compact",
        streamQuality: "lossless-plus",
        audioSourceId: "invalid source!",
        volume: 4,
        playbackMode: "repeat-one",
      }),
    ).toEqual({
      locale: "en",
      theme: "system",
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

  it("accepts Simplified Chinese as a locale preference", () => {
    expect(parseUiPreferences({ locale: "zh-CN" }).locale).toBe("zh-CN");
  });

  it("divides override themes into bright and dark groups", () => {
    const groupedThemes = THEME_GROUPS.flatMap((group) => group.options);
    const overrideThemes = THEME_OPTIONS.filter((theme) => theme.category !== null);

    expect(THEME_GROUPS.map((group) => group.value)).toEqual(["bright", "dark"]);
    expect(new Set(groupedThemes)).toEqual(new Set(overrideThemes));
    expect(groupedThemes).toHaveLength(overrideThemes.length);
  });

  it("keeps system and cover-driven themes outside the fixed theme groups", () => {
    expect(THEME_MODE_OPTIONS.map((theme) => theme.value)).toEqual(["system", "dynamic"]);
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
        locale: "zh-CN",
        theme: "dark",
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
        locale: "zh-CN",
        theme: "dark",
        streamQuality: "flac",
        audioSourceId: "imported-lx-source",
        volume: 0,
        playbackMode: "shuffle",
      }),
    );
  });
});
