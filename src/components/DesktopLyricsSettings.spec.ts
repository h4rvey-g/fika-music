import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import DesktopLyricsSettings from "./DesktopLyricsSettings.vue";
import { DEFAULT_DESKTOP_LYRICS_PREFERENCES } from "../lib/desktop-lyrics";

describe("DesktopLyricsSettings", () => {
  it("emits focused preference patches from its controls", async () => {
    const wrapper = mount(DesktopLyricsSettings, {
      props: { preferences: { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES } },
    });

    await wrapper.get('input[aria-label="Show desktop lyrics"]').setValue(true);
    await wrapper.get('input[aria-label="Current lyric color"]').setValue("#22cc88");
    await wrapper.get('button[aria-label="Align desktop lyrics right"]').trigger("click");

    expect(wrapper.emitted("update")).toEqual([
      [{ enabled: true }],
      [{ activeColor: "#22cc88" }],
      [{ alignment: "right" }],
    ]);
  });

  it("renders the configured colors and optional next line in the preview", () => {
    const wrapper = mount(DesktopLyricsSettings, {
      props: {
        preferences: {
          ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
          activeColor: "#123456",
          inactiveColor: "#abcdef",
          showNextLine: false,
        },
      },
    });

    const preview = wrapper.get('[aria-label="Desktop lyrics preview"]');
    expect(preview.text()).toContain("Coffee cools, the melody stays");
    expect(preview.text()).not.toContain("Another quiet song begins");
    expect(preview.get(".desktop-lyric-preview-fill").attributes("style"))
      .toContain("rgb(18, 52, 86)");
    expect(preview.get(".desktop-lyric-preview-fill").attributes("style"))
      .toContain("rgb(171, 205, 239)");
  });

  it("renders the selected text effect and updates its contrast in the preview", async () => {
    const wrapper = mount(DesktopLyricsSettings, {
      props: {
        preferences: {
          ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
          activeColor: "#111827",
          effect: "outline",
        },
      },
    });

    const effectLayer = wrapper.get('[data-testid="desktop-lyrics-preview-effect"]');
    expect(effectLayer.attributes("data-text-effect")).toBe("outline");
    expect(effectLayer.classes()).toContain("desktop-lyric-text-effect-outline");
    expect(effectLayer.find(".desktop-lyric-text-outline").exists()).toBe(true);
    expect(effectLayer.classes()).not.toContain("desktop-lyric-text-effect-shadow");
    expect(effectLayer.attributes("style")).toContain("rgb(255 255 255 / 88%)");

    await wrapper.setProps({
      preferences: {
        ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
        activeColor: "#f8fafc",
        effect: "outline",
      },
    });
    expect(effectLayer.attributes("style")).toContain("rgb(0 0 0 / 82%)");
  });
});
