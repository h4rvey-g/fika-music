import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRequestOutcome } from "./plugin-api";
import {
  cancelKugouQrLogin,
  getKugouPlaylist,
  getKugouRecommendations,
  pollKugouQrLogin,
  resolveKugouTrack,
} from "./kugou-api";
import { createKugouTrack } from "../test/fixtures";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const accountRef = "kugou-account:00000000-0000-4000-8000-000000000001";
const track = createKugouTrack();

describe("KuGou API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("polls and cancels QR login with camelCase command arguments", async () => {
    invokeMock.mockResolvedValue({ status: "waitingForScan", account: null });

    await pollKugouQrLogin("qr-session");
    await cancelKugouQrLogin("qr-session");

    expect(invokeMock.mock.calls).toEqual([
      ["poll_kugou_qr_login", { sessionId: "qr-session" }],
      ["cancel_kugou_qr_login", { sessionId: "qr-session" }],
    ]);
  });

  it("dispatches recommendation requests through the bundled plugin", async () => {
    invokeMock.mockResolvedValue({
      response: { action: "musicRecommendations", data: { list: [track] } },
      diagnostics: [],
    } satisfies SourceRequestOutcome);

    await expect(
      getKugouRecommendations(accountRef, "request-1"),
    ).resolves.toEqual({ data: [track], diagnostics: [] });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.kugou",
      request: {
        action: "musicRecommendations",
        source: "kg",
        accountRef,
        kind: "daily",
        limit: 50,
      },
      requestId: "request-1",
    });
  });

  it("dispatches Playlist reads using global collection ids", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "playlistRead",
        data: {
          playlist: {
            id: "collection_3_42_1_0",
            name: "Daily",
            description: null,
            coverUrl: null,
            trackCount: 1,
            ownerName: "Fika",
            canMutate: false,
          },
          tracks: [track],
        },
      },
      diagnostics: [],
    } satisfies SourceRequestOutcome);

    await getKugouPlaylist(accountRef, "collection_3_42_1_0", "request-2");

    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.kugou",
      request: {
        action: "playlistRead",
        source: "kg",
        accountRef,
        playlistId: "collection_3_42_1_0",
      },
      requestId: "request-2",
    });
  });

  it("resolves playback with account and KuGou track metadata", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "musicUrl",
        data: "https://fsandroid.kugou.com/preview.mp3",
      },
      diagnostics: [],
    } satisfies SourceRequestOutcome);

    await expect(
      resolveKugouTrack(track, "flac", accountRef, "request-3"),
  ).resolves.toMatchObject({
    track,
    providerName: "KuGou Music",
  });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.kugou",
      request: {
        action: "musicUrl",
        source: "kg",
        musicInfo: {
          hash: track.id,
          id: track.id,
          accountRef,
        },
        quality: "flac",
      },
      requestId: "request-3",
    });
  });
});
