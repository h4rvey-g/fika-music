import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNeteaseWebLogin } from "./netease-web-login";
import { createSourceAccount } from "../test/fixtures";
import type { NeteaseWebLoginPoll, NeteaseWebLoginStart } from "../lib/netease-api";

const api = vi.hoisted(() => ({
  startNeteaseWebLogin: vi.fn(),
  pollNeteaseWebLogin: vi.fn(),
  cancelNeteaseWebLogin: vi.fn(),
}));
vi.mock("../lib/netease-api", () => api);

const session = { sessionId: "web-session", expiresAt: 600 };

describe("official website login session", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    api.startNeteaseWebLogin.mockResolvedValue(session);
    api.pollNeteaseWebLogin.mockResolvedValue({ status: "waiting", account: null });
    api.cancelNeteaseWebLogin.mockResolvedValue(undefined);
  });
  afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

  function createSession() {
    const onConnected = vi.fn().mockResolvedValue(undefined);
    const onError = vi.fn();
    return { ...useNeteaseWebLogin({ onConnected, onError }), onConnected, onError };
  }

  it("polls without submitting credentials and consumes a confirmed account once", async () => {
    const account = createSourceAccount();
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(1_000);
    api.pollNeteaseWebLogin.mockResolvedValue({ status: "connected", account });
    await vi.advanceTimersByTimeAsync(1_000);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(login.onConnected).toHaveBeenCalledExactlyOnceWith(account);
    expect(api.pollNeteaseWebLogin).toHaveBeenCalledTimes(2);
    expect(login.login.value).toBeNull();
  });

  it("cancels a late-created window after the user dismissed login", async () => {
    let resolve!: (value: NeteaseWebLoginStart) => void;
    api.startNeteaseWebLogin.mockReturnValue(new Promise((done) => { resolve = done; }));
    const login = createSession();
    const starting = login.start();
    login.cancel();
    resolve(session);
    await starting;
    expect(api.cancelNeteaseWebLogin).toHaveBeenCalledWith("web-session");
    expect(login.login.value).toBeNull();
    expect(api.pollNeteaseWebLogin).not.toHaveBeenCalled();
  });

  it("ignores a completed poll after cancellation", async () => {
    let resolve!: (value: NeteaseWebLoginPoll) => void;
    api.pollNeteaseWebLogin.mockReturnValue(new Promise((done) => { resolve = done; }));
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(1_000);
    login.cancel();
    resolve({ status: "connected", account: createSourceAccount() });
    await vi.advanceTimersByTimeAsync(0);
    expect(login.onConnected).not.toHaveBeenCalled();
    expect(login.onError).not.toHaveBeenCalled();
  });

  it("reports expiry and stops polling", async () => {
    api.pollNeteaseWebLogin.mockResolvedValue({ status: "expired", account: null });
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(3_000);
    expect(login.onError).toHaveBeenCalledWith(new Error("Official website login expired. Sign in again."));
    expect(api.pollNeteaseWebLogin).toHaveBeenCalledTimes(1);
  });

  it("handles a closed native window without an error", async () => {
    api.pollNeteaseWebLogin.mockResolvedValue({ status: "cancelled", account: null });
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(login.login.value).toBeNull();
    expect(login.onError).not.toHaveBeenCalled();
  });

  it("closes the host window when importing its session fails", async () => {
    const error = new Error("fixture verification failure");
    api.pollNeteaseWebLogin.mockRejectedValue(error);
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(api.cancelNeteaseWebLogin).toHaveBeenCalledWith("web-session");
    expect(login.onError).toHaveBeenCalledWith(error);
  });

  it("ignores a failed poll after cancellation", async () => {
    let reject!: (error: Error) => void;
    api.pollNeteaseWebLogin.mockReturnValue(new Promise((_, fail) => { reject = fail; }));
    const login = createSession();
    await login.start();
    await vi.advanceTimersByTimeAsync(1_000);
    login.cancel();
    reject(new Error("fixture late failure"));
    await vi.advanceTimersByTimeAsync(0);
    expect(login.onError).not.toHaveBeenCalled();
  });
});
