import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_UI_PREFERENCES,
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
        volume: 4,
      }),
    ).toEqual({
      theme: "system",
      density: "compact",
      streamQuality: "128k",
      volume: 1,
    });
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
        volume: -1,
      },
      storage,
    );

    expect(storage.setItem).toHaveBeenCalledWith(
      UI_PREFERENCES_STORAGE_KEY,
      JSON.stringify({
        theme: "dark",
        density: "comfortable",
        streamQuality: "flac",
        volume: 0,
      }),
    );
  });
});
