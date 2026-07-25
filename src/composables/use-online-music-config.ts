import { useQueryClient, type QueryClient } from "@tanstack/vue-query";
import type { OnlineChannel, OnlineMusicSettings } from "../generated/bindings";
import {
  getOnlineMusicSettings,
  invalidateOnlinePlaybackCaches,
  listOnlineMusicChannels,
} from "../lib/online-music-api";

const settingsKey = ["online-music", "settings"] as const;
const channelsKey = ["online-music", "channels"] as const;

export type OnlineMusicConfig = {
  settings: OnlineMusicSettings;
  channels: OnlineChannel[];
};

export function createOnlineMusicConfig(queryClient: QueryClient) {
  async function load(): Promise<OnlineMusicConfig> {
    const [settings, channels] = await Promise.all([
      queryClient.fetchQuery({
        queryKey: settingsKey,
        queryFn: getOnlineMusicSettings,
        staleTime: Infinity,
      }),
      queryClient.fetchQuery({
        queryKey: channelsKey,
        queryFn: () => listOnlineMusicChannels(),
        staleTime: Infinity,
      }),
    ]);
    return { settings, channels };
  }

  function updateSettings(settings: OnlineMusicSettings): void {
    queryClient.setQueryData(settingsKey, settings);
    queryClient.removeQueries({ queryKey: channelsKey, exact: true });
    invalidateOnlinePlaybackCaches();
  }

  function invalidateChannels(): void {
    queryClient.removeQueries({ queryKey: channelsKey, exact: true });
    invalidateOnlinePlaybackCaches();
  }

  return { load, updateSettings, invalidateChannels };
}

export function useOnlineMusicConfig() {
  return createOnlineMusicConfig(useQueryClient());
}
