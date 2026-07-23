import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type { RemoteMediaSource } from "../generated/bindings";
import {
  dispatchPluginRequest,
  type PluginRecord,
  type SourceQuality,
} from "./plugin-api";

export type AudioSourceOption = {
  value: string;
  label: string;
};

export const AUDIO_SOURCE_OPTIONS: readonly AudioSourceOption[] = [
  { value: "nianxin", label: "念心音源" },
  { value: "changqing", label: "长青音源" },
] as const;

export type AudioSourceFamily = string;

export type AudioSourceTrackRequest = {
  family: AudioSourceFamily;
  source: string;
  trackId: string;
  quality: SourceQuality;
  requestId?: string;
};

export function isAudioSourceFamily(value: unknown): value is AudioSourceFamily {
  return (
    typeof value === "string" &&
    (AUDIO_SOURCE_OPTIONS.some((option) => option.value === value) ||
      importedPluginId(value) !== null)
  );
}

export function buildAudioSourceOptions(plugins: PluginRecord[]): AudioSourceOption[] {
  const imported = plugins
    .filter((plugin) => plugin.enabled && plugin.state === "enabled")
    .filter((plugin) =>
      plugin.providers.some(
        (provider) =>
          provider.initialized &&
          provider.entrypoint.startsWith("builtin:lx-js:") &&
          provider.sources.some((source) => source.actions.includes("musicUrl")),
      ),
    )
    .map((plugin) => ({
      value: `plugin:${plugin.id}`,
      label: plugin.name,
    }));
  return [...AUDIO_SOURCE_OPTIONS, ...imported];
}

export function audioSourceLabel(
  family: AudioSourceFamily,
  options: readonly AudioSourceOption[] = AUDIO_SOURCE_OPTIONS,
) {
  return options.find((option) => option.value === family)?.label ?? family;
}

export async function resolveAudioSourceTrack(
  request: AudioSourceTrackRequest,
): Promise<RemoteMediaSource> {
  const pluginId = importedPluginId(request.family);
  if (pluginId) {
    const outcome = await dispatchPluginRequest(
      pluginId,
      {
        action: "musicUrl",
        source: request.source,
        musicInfo: { id: request.trackId },
        quality: request.quality,
      },
      request.requestId,
    );
    if (outcome.response.action !== "musicUrl") {
      throw new Error("Imported audio source returned an unexpected response.");
    }
    return {
      url: outcome.response.data,
      mimeType: remoteMimeType(outcome.response.data, request.quality),
      diagnostics: outcome.diagnostics,
    };
  }

  return invoke<RemoteMediaSource>(TAURI_COMMANDS.resolveImportedLxTemplateMusicUrl, {
    family: request.family,
    source: request.source,
    trackId: request.trackId,
    quality: request.quality,
    requestId: request.requestId,
  });
}

function importedPluginId(value: string): string | null {
  const pluginId = value.startsWith("plugin:") ? value.slice("plugin:".length) : "";
  return /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(pluginId) ? pluginId : null;
}

function remoteMimeType(url: string, quality: SourceQuality) {
  const pathname = url.split(/[?#]/, 1)[0]?.toLowerCase() ?? "";
  if (pathname.endsWith(".flac")) {
    return "audio/flac";
  }
  if (pathname.endsWith(".mp3")) {
    return "audio/mpeg";
  }
  if (pathname.endsWith(".m4a") || pathname.endsWith(".mp4")) {
    return "audio/mp4";
  }
  if (pathname.endsWith(".aac")) {
    return "audio/aac";
  }
  if (pathname.endsWith(".ogg") || pathname.endsWith(".opus")) {
    return "audio/ogg";
  }
  if (quality === "flac" || quality === "flac24bit") {
    return "audio/flac";
  }
  return "audio/mpeg";
}
