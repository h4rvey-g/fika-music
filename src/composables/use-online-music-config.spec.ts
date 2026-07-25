import { beforeEach, describe, expect, it, vi } from "vitest";
import { createOnlineMusicConfig } from "./use-online-music-config";
import { createOnlineMusicSettings } from "../test/fixtures";
import { createTestQueryClient } from "../test/query-client";

const api = vi.hoisted(() => ({
  getOnlineMusicSettings: vi.fn(),
  invalidateOnlinePlaybackCaches: vi.fn(),
  listOnlineMusicChannels: vi.fn(),
}));

vi.mock("../lib/online-music-api", () => api);

const settings = createOnlineMusicSettings();

describe("online music config", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getOnlineMusicSettings.mockResolvedValue(settings);
    api.listOnlineMusicChannels.mockResolvedValue([]);
  });

  it("reuses settings and channels across track changes", async () => {
    const config = createOnlineMusicConfig(createTestQueryClient());

    await config.load();
    await config.load();

    expect(api.getOnlineMusicSettings).toHaveBeenCalledTimes(1);
    expect(api.listOnlineMusicChannels).toHaveBeenCalledTimes(1);
  });

  it("keeps updated settings and reloads derived channels", async () => {
    const config = createOnlineMusicConfig(createTestQueryClient());
    await config.load();
    const updated = { ...settings, excludedChannels: ["netease"] };

    config.updateSettings(updated);
    const result = await config.load();

    expect(result.settings).toEqual(updated);
    expect(api.getOnlineMusicSettings).toHaveBeenCalledTimes(1);
    expect(api.listOnlineMusicChannels).toHaveBeenCalledTimes(2);
    expect(api.invalidateOnlinePlaybackCaches).toHaveBeenCalledTimes(1);
  });
});
