import { describe, expect, it, vi } from "vitest";
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
