import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "../i18n";
import { DEFAULT_GLOBAL_SHORTCUT_PREFERENCES } from "../lib/global-shortcut-preferences";
import SystemShortcutSettings from "./SystemShortcutSettings.vue";

const emptyErrors = {
  togglePlayback: null,
  previousTrack: null,
  nextTrack: null,
  seekBackward: null,
  seekForward: null,
  volumeDown: null,
  volumeUp: null,
  toggleMute: null,
};

describe("system shortcut settings", () => {
  beforeEach(() => setLocale("en"));

  it("renders disabled-by-default actions and starts recording", async () => {
    const wrapper = mount(SystemShortcutSettings, {
      props: {
        applyingAction: null,
        available: true,
        bindings: { ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES },
        captureError: null,
        errors: { ...emptyErrors },
        recordingAction: null,
      },
    });

    expect(wrapper.text()).toContain("System shortcuts");
    expect(wrapper.text()).toContain("Play or pause");
    expect(wrapper.text()).toContain("Not set");
    await wrapper.get('button[aria-label="Record system shortcut for Play or pause"]').trigger("click");
    expect(wrapper.emitted("record")).toEqual([["togglePlayback"]]);
  });

  it("shows an assigned binding, registration error, and clear command", async () => {
    const wrapper = mount(SystemShortcutSettings, {
      props: {
        applyingAction: null,
        available: true,
        bindings: {
          ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
          toggleMute: "CommandOrControl+Shift+KeyM",
        },
        captureError: null,
        errors: {
          ...emptyErrors,
          toggleMute: { code: "unavailable" },
        },
        recordingAction: null,
      },
    });

    expect(wrapper.text()).toContain("Ctrl+Shift+M");
    expect(wrapper.text()).toContain("This shortcut is unavailable");
    await wrapper.get('button[aria-label="Clear system shortcut for Mute or unmute"]').trigger("click");
    expect(wrapper.emitted("clear")).toEqual([["toggleMute"]]);
  });
});
