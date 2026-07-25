import { afterEach, describe, expect, it, vi } from "vitest";
import { useQrLoginSession, useSourcePlaybackRequest } from "./source-workspace";

const pluginApi = vi.hoisted(() => ({ cancelSourceRequest: vi.fn() }));
vi.mock("../lib/plugin-api", () => pluginApi);

describe("source workspace lifecycle", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.resetAllMocks();
  });

  it("polls QR login through confirmation and connection", async () => {
    vi.useFakeTimers();
    const onConnected = vi.fn(async () => undefined);
    const poll = vi
      .fn()
      .mockResolvedValueOnce({ status: "waitingForConfirmation", account: null })
      .mockResolvedValueOnce({
        status: "connected",
        account: { accountRef: "account-1", displayName: "Fika" },
      });
    const session = useQrLoginSession({
      providerName: "Music Provider",
      start: async () => ({ sessionId: "session-1" }),
      poll,
      cancel: async () => undefined,
      onConnected,
      onError: vi.fn(),
      pollIntervalMs: 10,
    });

    await session.start();
    await vi.advanceTimersByTimeAsync(10);
    expect(session.status.value).toBe("Confirm in Music Provider");
    await vi.advanceTimersByTimeAsync(10);

    expect(onConnected).toHaveBeenCalledWith({
      accountRef: "account-1",
      displayName: "Fika",
    });
  });

  it("cancels the active playback request when abandoned", async () => {
    pluginApi.cancelSourceRequest.mockResolvedValue(true);
    const playback = useSourcePlaybackRequest();
    let finish: (() => void) | undefined;
    const running = playback.run("track-1", (requestId) =>
      new Promise<string>((resolve) => {
        finish = () => resolve(requestId);
      }),
    );

    playback.abandon();
    finish?.();
    await running;

    expect(pluginApi.cancelSourceRequest).toHaveBeenCalledWith(expect.any(String));
    expect(playback.activeTrackId.value).toBeNull();
  });
});
