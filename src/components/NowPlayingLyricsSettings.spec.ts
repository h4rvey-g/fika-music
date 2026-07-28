import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import NowPlayingLyricsSettings from "./NowPlayingLyricsSettings.vue";
import {
  DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
  NOW_PLAYING_LYRICS_THEME_COLOR,
} from "../lib/now-playing-lyrics";

describe("NowPlayingLyricsSettings", () => {
  it("emits focused preference patches from its controls", async () => {
    const wrapper = mount(NowPlayingLyricsSettings, {
      props: { preferences: { ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES } },
    });

    await wrapper.get('select[aria-label="Now playing lyric typeface"]').setValue("serif");
    await wrapper.get('select[aria-label="Current lyric weight"]').setValue("700");
    await wrapper.get('input[aria-label="Now playing lyric size"]').setValue("22");
    await wrapper.get('input[aria-label="Now playing lyric line spacing"]').setValue("20");
    await wrapper.get('button[aria-label="Align now playing lyrics right"]').trigger("click");
    await wrapper.get('input[aria-label="Other lyric opacity"]').setValue("0.7");
    await wrapper.get('input[aria-label="Current lyric color"]').setValue("#123456");
    await wrapper.get('button[aria-label="Use theme color for current lyric"]').trigger("click");
    await wrapper.findAll("button").find((button) => (
      button.text().includes("Reset now playing lyrics")
    ))?.trigger("click");

    expect(wrapper.emitted("update")).toEqual([
      [{ font: "serif" }],
      [{ activeFontWeight: 700 }],
      [{ fontSize: 22 }],
      [{ lineGap: 20 }],
      [{ alignment: "right" }],
      [{ inactiveOpacity: 0.7 }],
      [{ activeColor: "#123456" }],
      [{ activeColor: NOW_PLAYING_LYRICS_THEME_COLOR }],
    ]);
    expect(wrapper.emitted("reset")).toHaveLength(1);
  });

  it("renders the selected typography and colors in the preview", () => {
    const wrapper = mount(NowPlayingLyricsSettings, {
      props: {
        preferences: {
          ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
          font: "monospace",
          fontSize: 20,
          lineGap: 12,
          activeFontWeight: 700,
          alignment: "right",
          activeColor: "#123456",
          inactiveColor: "#abcdef",
          inactiveOpacity: 0.3,
        },
      },
    });

    const preview = wrapper.get('[aria-label="Now playing lyrics preview"]');
    const previewLines = preview.findAll("p");
    const activeLine = previewLines[1].element as HTMLElement;
    const inactiveLine = previewLines[0].element as HTMLElement;
    const textContainer = preview.get("div").element as HTMLElement;

    expect(textContainer.style.fontFamily).toContain("ui-monospace");
    expect(textContainer.style.textAlign).toBe("right");
    expect(activeLine.style.color).toBe("rgb(18, 52, 86)");
    expect(activeLine.style.fontSize).toBe("20px");
    expect(activeLine.style.fontWeight).toBe("700");
    expect(activeLine.style.paddingBlock).toBe("6px");
    expect(inactiveLine.style.color).toBe("rgb(171, 205, 239)");
    expect(inactiveLine.style.opacity).toBe("0.3");
  });
});
