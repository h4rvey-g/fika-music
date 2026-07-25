import { afterEach, describe, expect, it, vi } from "vitest";
import { firstSuccessfulWithTimeout } from "./async-utils";

describe("firstSuccessfulWithTimeout", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("clears its timer after the first successful result", async () => {
    vi.useFakeTimers();

    await expect(
      firstSuccessfulWithTimeout([Promise.resolve("ready")], 10_000),
    ).resolves.toBe("ready");

    expect(vi.getTimerCount()).toBe(0);
  });

  it("removes the abort listener after all candidates fail", async () => {
    const controller = new AbortController();
    const removeEventListener = vi.spyOn(controller.signal, "removeEventListener");

    await expect(
      firstSuccessfulWithTimeout([Promise.reject(new Error("failed"))], 10_000, controller.signal),
    ).rejects.toThrow("All playback candidates failed.");

    expect(removeEventListener).toHaveBeenCalledWith("abort", expect.any(Function));
  });

  it("rejects immediately when its signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      firstSuccessfulWithTimeout([new Promise(() => undefined)], 10_000, controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});
