import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App.vue";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
}));

vi.mock("./components/PluginManager.vue", () => ({
  default: {
    name: "PluginManager",
    template: '<div data-testid="plugin-manager">Plugin manager</div>',
  },
}));

vi.mock("./components/NeteaseSource.vue", () => ({
  default: {
    name: "NeteaseSource",
    template: '<div data-testid="netease-source">NetEase source</div>',
  },
}));

describe("application shell", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    tauriMocks.listen.mockResolvedValue(vi.fn());
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "list_local_tracks") {
        return Promise.resolve([]);
      }
      if (command === "get_scan_status") {
        return Promise.resolve({
          isRunning: false,
          folderPath: null,
          discoveredFiles: 0,
          scannedFiles: 0,
          indexedTracks: 0,
          skippedFiles: 0,
          errorCount: 0,
          lastError: null,
          startedAt: null,
          finishedAt: null,
        });
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
  });

  it("navigates between all sidebar sections and closes the mobile drawer", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    expect(navigation.findAll("button").map((button) => button.text())).toEqual([
      "Local Music",
      "Audio Sources",
      "Plugins",
      "Settings",
    ]);
    expect(wrapper.get("h1").text()).toBe("Local Music");
    const playbackBar = wrapper.get('footer[aria-label="Playback bar"]');
    expect(playbackBar.text()).toContain("Nothing playing");

    const drawerToggle = wrapper.get<HTMLInputElement>("#app-sidebar");
    await drawerToggle.setValue(true);
    const settingsButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Settings");
    expect(settingsButton).toBeDefined();
    await settingsButton?.trigger("click");

    expect(wrapper.get("h1").text()).toBe("Settings");
    expect(wrapper.find("#theme-preference").exists()).toBe(true);
    expect(wrapper.get('footer[aria-label="Playback bar"]').element).toBe(playbackBar.element);
    expect(settingsButton?.attributes("aria-current")).toBe("page");
    expect(drawerToggle.element.checked).toBe(false);
    wrapper.unmount();
  });
});
