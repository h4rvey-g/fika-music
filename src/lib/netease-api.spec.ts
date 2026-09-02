import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRequestOutcome } from "./plugin-api";
import {
  addNeteasePlaylistTrack,
  cancelNeteasePhoneLogin,
  cancelNeteaseQrLogin,
  completeNeteasePhoneLogin,
  getNeteaseRecommendations,
  pollNeteaseQrLogin,
  resolveNeteaseTrack,
  startNeteasePhoneLogin,
} from "./netease-api";
import { createNeteaseTrack } from "../test/fixtures";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const track = createNeteaseTrack();
const accountRef = "netease-account:00000000-0000-4000-8000-000000000001";

describe("NetEase API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("polls QR login with camelCase command arguments", async () => {
    invokeMock.mockResolvedValue({ status: "waitingForScan", account: null });

    await pollNeteaseQrLogin("qr-session");

    expect(invokeMock).toHaveBeenCalledWith("poll_netease_qr_login", {
      sessionId: "qr-session",
    });

    await cancelNeteaseQrLogin("qr-session");
    expect(invokeMock).toHaveBeenLastCalledWith("cancel_netease_qr_login", {
      sessionId: "qr-session",
    });
  });

  it("uses an opaque session for verification-code login", async () => {
    invokeMock.mockResolvedValue({ sessionId: "phone-session", expiresAt: 600 });

    await startNeteasePhoneLogin("13800138000");
    await completeNeteasePhoneLogin("phone-session", "123456");
    await cancelNeteasePhoneLogin("phone-session");

    expect(invokeMock.mock.calls).toEqual([
      ["start_netease_phone_login", { phone: "13800138000" }],
      ["complete_netease_phone_login", {
        sessionId: "phone-session",
        verificationCode: "123456",
      }],
      ["cancel_netease_phone_login", { sessionId: "phone-session" }],
    ]);
  });

  it("dispatches recommendation requests through the bundled Plugin", async () => {
    const outcome: SourceRequestOutcome = {
      response: { action: "musicRecommendations", data: { list: [track] } },
      diagnostics: [],
    };
    invokeMock.mockResolvedValue(outcome);

    await expect(getNeteaseRecommendations(accountRef, "request-1")).resolves.toEqual({
      data: [track],
      diagnostics: [],
    });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.netease",
      request: {
        action: "musicRecommendations",
        source: "wy",
        accountRef,
        kind: "daily",
        limit: 50,
      },
      requestId: "request-1",
    });
  });

  it("sends explicit track ownership on playlist mutations", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "playlistAddTrack",
        data: {
          auditId: 1,
          operation: "add",
          playlistId: "playlist-1",
          trackId: track.id,
          occurredAt: 1,
        },
      },
      diagnostics: [],
    } satisfies SourceRequestOutcome);

    await addNeteasePlaylistTrack(accountRef, "playlist-1", track);

    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.netease",
      request: {
        action: "playlistAddTrack",
        source: "wy",
        accountRef,
        playlistId: "playlist-1",
        track: { id: track.id, source: track.source },
      },
      requestId: undefined,
    });
  });

  it("resolves account-backed FLAC playback without exposing a credential", async () => {
    invokeMock.mockResolvedValue({
      response: {
        action: "musicUrl",
        data: "https://example.test/Test%20Track.flac?token=short-lived",
      },
      diagnostics: [],
    } satisfies SourceRequestOutcome);

  await expect(
    resolveNeteaseTrack(track, "flac", accountRef, "request-2"),
  ).resolves.toMatchObject({
    providerName: "NetEase Cloud Music",
  });
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.netease",
      request: {
        action: "musicUrl",
        source: "wy",
        musicInfo: { id: track.id, accountRef },
        quality: "flac",
      },
      requestId: "request-2",
    });
  });
});
