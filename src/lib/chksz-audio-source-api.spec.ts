import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearChkszApiKey,
  getChkszApiKeyStatus,
  setChkszApiKey,
} from "./chksz-audio-source-api";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("ChKSz Audio Source API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("reads only API key configuration status", async () => {
    invokeMock.mockResolvedValue(true);

    await expect(getChkszApiKeyStatus()).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("get_chksz_api_key_status");
  });

  it("sets and clears the API key through backend commands", async () => {
    invokeMock.mockResolvedValue(undefined);

    await setChkszApiKey("test-api-key");
    await clearChkszApiKey();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "set_chksz_api_key", {
      apiKey: "test-api-key",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "clear_chksz_api_key");
  });
});
