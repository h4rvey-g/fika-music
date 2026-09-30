import { flushPromises } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  usePhoneLoginSession,
  useQrLoginSession,
  useSourcePlaybackRequest,
} from "./source-workspace";

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

  it("keeps the QR visible when risk control rejects its first poll", async () => {
    vi.useFakeTimers();
    const error = {
      code: "verification-required",
      message: "Music Provider requires security verification",
    };
    const poll = vi.fn()
      .mockRejectedValueOnce(error)
      .mockResolvedValue({ status: "waitingForScan", account: null });
    const cancel = vi.fn(async () => undefined);
    const onError = vi.fn();
    const session = useQrLoginSession({
      providerName: "Music Provider",
      start: async () => ({ sessionId: "session-1", qrImageDataUrl: "fixture-qr" }),
      poll,
      cancel,
      onConnected: vi.fn(async () => undefined),
      onError,
      pollIntervalMs: 10,
      pauseOnError: (failure) => failure === error,
    });

    await session.start();
    await vi.advanceTimersByTimeAsync(10);

    expect(onError).toHaveBeenCalledWith(error);
    expect(session.login.value).toMatchObject({ sessionId: "session-1" });
    expect(session.isPaused.value).toBe(true);
    expect(cancel).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(100);
    expect(poll).toHaveBeenCalledTimes(1);
    session.checkAgain();
    await flushPromises();
    expect(poll).toHaveBeenCalledTimes(2);
    expect(session.isPaused.value).toBe(false);
    await vi.advanceTimersByTimeAsync(10);
    expect(poll).toHaveBeenCalledTimes(3);
    expect(session.login.value).toMatchObject({ sessionId: "session-1" });
    expect(onError).toHaveBeenCalledTimes(1);
    session.cancel();
    expect(cancel).toHaveBeenCalledWith("session-1");
  });

  it("cancels the QR session on unrelated polling errors", async () => {
    vi.useFakeTimers();
    const cancel = vi.fn(async () => undefined);
    const session = useQrLoginSession({
      providerName: "Music Provider",
      start: async () => ({ sessionId: "session-1" }),
      poll: vi.fn().mockRejectedValue(new Error("network unavailable")),
      cancel,
      onConnected: vi.fn(async () => undefined),
      onError: vi.fn(),
      pollIntervalMs: 10,
      pauseOnError: () => false,
    });

    await session.start();
    await vi.advanceTimersByTimeAsync(10);
    expect(session.login.value).toBeNull();
    expect(cancel).toHaveBeenCalledWith("session-1");
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

  it("completes a verification-code login through one short-lived session", async () => {
    const onConnected = vi.fn(async () => undefined);
    const complete = vi.fn(async () => ({
      accountRef: "account-1",
      displayName: "Fika",
    }));
    const session = usePhoneLoginSession({
      start: async () => ({ sessionId: "phone-session" }),
      complete,
      cancel: async () => undefined,
      onConnected,
      onError: vi.fn(),
    });

    await session.sendCode("13800138000");
    await session.complete("123456");

    expect(complete).toHaveBeenCalledWith("phone-session", "123456");
    expect(onConnected).toHaveBeenCalledWith({
      accountRef: "account-1",
      displayName: "Fika",
    });
    expect(session.login.value).toBeNull();
    session.cancel();
  });
});
