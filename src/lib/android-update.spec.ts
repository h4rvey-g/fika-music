import { afterEach, describe, expect, it, vi } from "vitest";
import { checkAndroidUpdate } from "./android-update";

function releaseResponse(overrides: Record<string, unknown> = {}) {
  return new Response(JSON.stringify({
    tag_name: "v0.3.2",
    published_at: "2026-09-02T10:54:22Z",
    body: "Android update",
    assets: [
      {
        name: "Fika-Music_0.3.2_arm64-v8a.apk",
        browser_download_url:
          "https://github.com/h4rvey-g/fika-music/releases/download/v0.3.2/Fika-Music_0.3.2_arm64-v8a.apk",
      },
      ...["armeabi-v7a", "x86", "x86_64"].map((abi) => ({
        name: `Fika-Music_0.3.2_${abi}.apk`,
        browser_download_url:
          `https://github.com/h4rvey-g/fika-music/releases/download/v0.3.2/Fika-Music_0.3.2_${abi}.apk`,
      })),
    ],
    ...overrides,
  }), { status: 200 });
}

describe("checkAndroidUpdate", () => {
  afterEach(() => vi.useRealTimers());

  it("preserves the HTTP status when GitHub rejects the request", async () => {
    await expect(checkAndroidUpdate({
      currentVersion: "0.3.4",
      architecture: "aarch64",
      fetcher: vi.fn(async () => new Response("Forbidden", { status: 403 })),
    })).rejects.toThrow("Unable to check for Android updates (GitHub HTTP 403).");
  });

  it.each([
    [403, { "x-ratelimit-remaining": "0" }],
    [429, {}],
  ])("identifies GitHub rate limiting for HTTP %s", async (status, headers) => {
    await expect(checkAndroidUpdate({
      currentVersion: "0.3.4",
      architecture: "aarch64",
      fetcher: vi.fn(async () => new Response("", { status, headers })),
    })).rejects.toThrow("GitHub update checks are rate limited. Try again later.");
  });

  it("distinguishes an unreadable installed version without requesting GitHub", async () => {
    const fetcher = vi.fn(async () => releaseResponse());
    await expect(checkAndroidUpdate({
      currentVersion: "",
      architecture: "aarch64",
      fetcher,
    })).rejects.toThrow("Unable to read the installed Android app version.");
    expect(fetcher).not.toHaveBeenCalled();
  });

  it.each(["null", "<html>Blocked</html>", '{}'])(
    "reports invalid release metadata for %s",
    async (body) => {
      await expect(checkAndroidUpdate({
        currentVersion: "0.3.4",
        architecture: "aarch64",
        fetcher: vi.fn(async () => new Response(body)),
      })).rejects.toThrow("GitHub returned invalid Android update metadata.");
    },
  );

  it("distinguishes a network failure from an HTTP rejection", async () => {
    await expect(checkAndroidUpdate({
      currentVersion: "0.3.4",
      architecture: "aarch64",
      fetcher: vi.fn(async () => { throw new TypeError("Failed to fetch"); }),
    })).rejects.toThrow("Unable to reach GitHub for Android updates. Check your connection and try again.");
  });

  it("keeps the timeout active while reading the release body", async () => {
    vi.useFakeTimers();
    let rejectBody: ((error: Error) => void) | undefined;
    const fetcher: typeof fetch = vi.fn(async (_input, options) => {
      return {
        ok: true,
        json: () => new Promise((_resolve, reject) => {
          rejectBody = reject;
          options?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
        }),
      } as Response;
    });
    const assertion = expect(checkAndroidUpdate({
      currentVersion: "0.3.4",
      architecture: "aarch64",
      fetcher,
    })).rejects.toThrow("Android update check timed out. Try again.");

    await vi.advanceTimersByTimeAsync(30_000);
    // Let the old implementation finish too, so the regression fails without hanging.
    rejectBody?.(new DOMException("Aborted", "AbortError"));
    await assertion;
  });

  it("returns the APK matching a newer release and the device architecture", async () => {
    const fetcher = vi.fn(async () => releaseResponse());

    const update = await checkAndroidUpdate({
      currentVersion: "0.3.1",
      architecture: "aarch64",
      fetcher,
    });

    expect(update).toEqual({
      version: "0.3.2",
      date: "2026-09-02T10:54:22Z",
      body: "Android update",
      downloadUrl:
        "https://github.com/h4rvey-g/fika-music/releases/download/v0.3.2/Fika-Music_0.3.2_arm64-v8a.apk",
    });
  });

  it("returns no update when the latest release matches the installed version", async () => {
    const fetcher = vi.fn(async () => releaseResponse());

    const update = await checkAndroidUpdate({
      currentVersion: "0.3.2",
      architecture: "aarch64",
      fetcher,
    });

    expect(update).toBeNull();
  });

  it.each([
    ["arm", "armeabi-v7a"],
    ["x86", "x86"],
    ["x86_64", "x86_64"],
  ])("maps the %s architecture to the %s APK", async (architecture, abi) => {
    const fetcher = vi.fn(async () => releaseResponse());

    const update = await checkAndroidUpdate({
      currentVersion: "0.3.1",
      architecture,
      fetcher,
    });

    expect(update?.downloadUrl).toContain(`Fika-Music_0.3.2_${abi}.apk`);
  });

  it("rejects a newer release without an APK for the device architecture", async () => {
    const fetcher = vi.fn(async () => releaseResponse({ assets: [] }));

    await expect(checkAndroidUpdate({
      currentVersion: "0.3.1",
      architecture: "aarch64",
      fetcher,
    })).rejects.toThrow("No Android update package is available for this device.");
  });
});
