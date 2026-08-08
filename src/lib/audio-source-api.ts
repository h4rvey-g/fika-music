import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  AudioSourceAvailability,
  AudioSourceRecord,
  SourceCapability,
  SourceDiagnostic,
  SourceQuality,
  SourceRequest,
  SourceRequestOutcome,
} from "../generated/bindings";

export type {
  AudioSourceAvailability,
  AudioSourceDiagnostic,
  AudioSourceRecord,
  AudioSourceState,
} from "../generated/bindings";

export type AudioSourceOption = {
  value: string;
  label: string;
};

export type AudioSourceId = string;

export type AudioSourceTrackRequest = {
  audioSourceId: AudioSourceId;
  source: string;
  trackId: string;
  musicInfo?: Record<string, unknown>;
  quality: SourceQuality;
  requestId?: string;
};

export type ResolvedAudioSourceTrack = {
  url: string;
  diagnostics: SourceDiagnostic[];
};

export function listAudioSources() {
  return invoke<AudioSourceRecord[]>(TAURI_COMMANDS.listAudioSources);
}

export function selectAudioSourceFile() {
  return invoke<string | null>(TAURI_COMMANDS.selectAudioSourceFile);
}

export function refreshAudioSources() {
  return invoke<AudioSourceRecord[]>(TAURI_COMMANDS.refreshAudioSources);
}

export function importAudioSource(sourcePath: string) {
  return invoke<AudioSourceRecord>(TAURI_COMMANDS.importAudioSource, { sourcePath });
}

export function importAudioSourceUrl(sourceUrl: string) {
  return invoke<AudioSourceRecord>(TAURI_COMMANDS.importAudioSourceUrl, { sourceUrl });
}

export function setAudioSourceCapabilities(
  audioSourceId: string,
  capabilities: SourceCapability[],
  reviewed: boolean,
) {
  return invoke<AudioSourceRecord>(TAURI_COMMANDS.setAudioSourceCapabilities, {
    audioSourceId,
    capabilities,
    reviewed,
  });
}

export function setAudioSourceEnabled(audioSourceId: string, enabled: boolean) {
  return invoke<AudioSourceRecord>(TAURI_COMMANDS.setAudioSourceEnabled, {
    audioSourceId,
    enabled,
  });
}

export function removeAudioSource(audioSourceId: string) {
  return invoke<AudioSourceRecord[]>(TAURI_COMMANDS.removeAudioSource, { audioSourceId });
}

export function clearAudioSourceDiagnostics(audioSourceId: string) {
  return invoke<AudioSourceRecord>(TAURI_COMMANDS.clearAudioSourceDiagnostics, {
    audioSourceId,
  });
}

export function checkAudioSourceAvailability(
  audioSourceId: string,
  sourceId?: string,
) {
  return invoke<AudioSourceAvailability[]>(TAURI_COMMANDS.checkAudioSourceAvailability, {
    audioSourceId,
    sourceId: sourceId ?? null,
  });
}

export function dispatchAudioSourceRequest(
  audioSourceId: string,
  request: SourceRequest,
  requestId?: string,
) {
  return invoke<SourceRequestOutcome>(TAURI_COMMANDS.dispatchAudioSourceRequest, {
    audioSourceId,
    request,
    requestId,
  });
}

export function isAudioSourceId(value: unknown): value is AudioSourceId {
  return (
    value === "" ||
    (typeof value === "string" &&
      value.length <= 128 &&
      /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value))
  );
}

export function buildAudioSourceOptions(
  audioSources: AudioSourceRecord[],
): AudioSourceOption[] {
  return audioSources
    .filter((audioSource) => audioSource.enabled && audioSource.state === "enabled")
    .filter((audioSource) =>
      audioSource.sources.some((source) => source.actions.includes("musicUrl")),
    )
    .map((audioSource) => ({
      value: audioSource.id,
      label: audioSource.name,
    }));
}

export function audioSourceLabel(
  audioSourceId: AudioSourceId,
  options: readonly AudioSourceOption[],
) {
  return options.find((option) => option.value === audioSourceId)?.label ?? "Audio source";
}

export async function resolveAudioSourceTrack(
  request: AudioSourceTrackRequest,
): Promise<ResolvedAudioSourceTrack> {
  if (!request.audioSourceId) {
    throw new Error("No audio source is configured.");
  }
  const outcome = await dispatchAudioSourceRequest(
    request.audioSourceId,
    {
      action: "musicUrl",
      source: request.source,
      musicInfo: {
        ...request.musicInfo,
        id: request.musicInfo?.id ?? request.trackId,
      },
      quality: request.quality,
    },
    request.requestId,
  );
  if (outcome.response.action !== "musicUrl") {
    throw new Error("Audio source returned an unexpected response.");
  }
  return {
    url: outcome.response.data,
    diagnostics: outcome.diagnostics,
  };
}
