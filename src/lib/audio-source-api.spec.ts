import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildAudioSourceOptions,
  resolveAudioSourceTrack,
} from "./audio-source-api";
import type { PluginRecord } from "./plugin-api";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("audio source API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("resolves a platform track through the selected imported audio source", async () => {
    invokeMock.mockResolvedValue({
      url: "https://cdn.example.test/track.mp3",
      mimeType: "audio/mpeg",
      diagnostics: [],
    });

    await resolveAudioSourceTrack({
      family: "changqing",
      source: "wy",
      trackId: "347230",
      quality: "320k",
      requestId: "request-1",
    });

    expect(invokeMock).toHaveBeenCalledWith("resolve_imported_lx_template_music_url", {
      family: "changqing",
      source: "wy",
      trackId: "347230",
      quality: "320k",
      requestId: "request-1",
    });
  });

  it("builds options from enabled imported LX plugins", () => {
    const importedPlugin = {
      id: "imported-lx-source",
      name: "Imported LX Source",
      state: "enabled",
      enabled: true,
      providers: [
        {
          initialized: true,
          entrypoint:
            "builtin:lx-js:static-templates:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          sources: [{ actions: ["musicUrl"] }],
        },
      ],
    } as PluginRecord;

    expect(buildAudioSourceOptions([importedPlugin])).toContainEqual({
      value: "plugin:imported-lx-source",
      label: "Imported LX Source",
    });
  });

  it("dispatches playback through an enabled imported LX plugin", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "musicUrl",
        data: "https://cdn.example.test/track.mp3",
      },
      diagnostics: [
        { sourceId: "imported-lx-source", level: "info", message: "resolved" },
      ],
    });

    await expect(
      resolveAudioSourceTrack({
        family: "plugin:imported-lx-source",
        source: "wy",
        trackId: "347230",
        quality: "320k",
        requestId: "request-2",
      }),
    ).resolves.toMatchObject({
      url: "https://cdn.example.test/track.mp3",
      mimeType: "audio/mpeg",
    });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "imported-lx-source",
      request: {
        action: "musicUrl",
        source: "wy",
        musicInfo: { id: "347230" },
        quality: "320k",
      },
      requestId: "request-2",
    });
  });
});
