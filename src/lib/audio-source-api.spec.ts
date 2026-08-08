import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildAudioSourceOptions,
  checkAudioSourceAvailability,
  importAudioSource,
  isAudioSourceId,
  listAudioSources,
  resolveAudioSourceTrack,
} from "./audio-source-api";
import { createAudioSourceRecord } from "../test/fixtures";

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

  it("uses dedicated list and import commands", async () => {
    invokeMock
      .mockResolvedValueOnce([createAudioSourceRecord()])
      .mockResolvedValueOnce(createAudioSourceRecord());

    await listAudioSources();
    await importAudioSource("/downloads/source.js");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "list_audio_sources");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "import_audio_source", {
      sourcePath: "/downloads/source.js",
    });
  });

  it("checks one source or the full catalog through the availability command", async () => {
    invokeMock.mockResolvedValue([]);

    await checkAudioSourceAvailability("imported-source", "wy");
    await checkAudioSourceAvailability("imported-source");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "check_audio_source_availability", {
      audioSourceId: "imported-source",
      sourceId: "wy",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "check_audio_source_availability", {
      audioSourceId: "imported-source",
      sourceId: null,
    });
  });

  it("builds playback options only from enabled audio source records", () => {
    expect(
      buildAudioSourceOptions([
        createAudioSourceRecord(),
        createAudioSourceRecord({ id: "disabled", name: "Disabled", enabled: false }),
      ]),
    ).toEqual([{ value: "imported-source", label: "Imported Source" }]);
  });

  it("dispatches musicUrl through the selected audio source", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "musicUrl",
        data: "https://cdn.example.test/track.mp3",
      },
      diagnostics: [
        { sourceId: "imported-source", level: "info", message: "resolved" },
      ],
    });

    await expect(
      resolveAudioSourceTrack({
        audioSourceId: "imported-source",
        source: "wy",
        trackId: "347230",
        musicInfo: { id: 1, songmid: "track-mid", name: "Test Track" },
        quality: "320k",
        requestId: "request-1",
      }),
    ).resolves.toMatchObject({
      url: "https://cdn.example.test/track.mp3",
    });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_audio_source_request", {
      audioSourceId: "imported-source",
      request: {
        action: "musicUrl",
        source: "wy",
        musicInfo: { id: 1, songmid: "track-mid", name: "Test Track" },
        quality: "320k",
      },
      requestId: "request-1",
    });
  });

  it("does not resolve playback without a configured source", async () => {
    await expect(
      resolveAudioSourceTrack({
        audioSourceId: "",
        source: "wy",
        trackId: "347230",
        quality: "320k",
      }),
    ).rejects.toThrow("No audio source is configured");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("rejects identifiers that cannot fit a managed package path", () => {
    expect(isAudioSourceId("a".repeat(129))).toBe(false);
  });
});
