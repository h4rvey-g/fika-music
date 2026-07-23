import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  NeteaseAccount,
  NeteaseMutationAudit,
  NeteaseQrLoginPoll,
  NeteaseQrLoginStart,
} from "../generated/bindings";
import {
  dispatchPluginRequest,
  type RemoteTrack,
  type SourceDiagnostic,
  type SourcePlaylist,
  type SourcePlaylistDetail,
  type SourcePlaylistMutation,
  type SourceQuality,
  type SourceRequest,
  type SourceRequestOutcome,
  type SourceResponse,
} from "./plugin-api";

export const NETEASE_PLUGIN_ID = "fika.netease";
export const NETEASE_SOURCE_ID = "wy";

export type {
  NeteaseAccount,
  NeteaseMutationAudit,
  NeteaseQrLoginPoll,
  NeteaseQrLoginStart,
} from "../generated/bindings";

export type NeteaseOperationResult<T> = {
  data: T;
  diagnostics: SourceDiagnostic[];
};

export type NeteasePlayback = {
  track: RemoteTrack;
  url: string;
  mimeType: string;
  providerName: string;
  diagnostics: SourceDiagnostic[];
};

export function startNeteaseQrLogin() {
  return invoke<NeteaseQrLoginStart>(TAURI_COMMANDS.startNeteaseQrLogin);
}

export function pollNeteaseQrLogin(sessionId: string) {
  return invoke<NeteaseQrLoginPoll>(TAURI_COMMANDS.pollNeteaseQrLogin, { sessionId });
}

export function cancelNeteaseQrLogin(sessionId: string) {
  return invoke<void>(TAURI_COMMANDS.cancelNeteaseQrLogin, { sessionId });
}

export function listNeteaseAccounts() {
  return invoke<NeteaseAccount[]>(TAURI_COMMANDS.listNeteaseAccounts);
}

export function disconnectNeteaseAccount(accountRef: string) {
  return invoke<void>(TAURI_COMMANDS.disconnectNeteaseAccount, { accountRef });
}

export function listNeteaseMutationAudit(accountRef?: string, limit = 50) {
  return invoke<NeteaseMutationAudit[]>(TAURI_COMMANDS.listNeteaseMutationAudit, {
    accountRef: accountRef ?? null,
    limit,
  });
}

export async function getNeteaseRecommendations(
  accountRef: string,
  requestId?: string,
): Promise<NeteaseOperationResult<RemoteTrack[]>> {
  const outcome = await dispatch({
    action: "musicRecommendations",
    source: NETEASE_SOURCE_ID,
    accountRef,
    limit: 50,
  }, requestId);
  return operationResult(outcome, "musicRecommendations", (response) => response.data.list);
}

export async function getNeteasePlaylists(
  accountRef: string,
  requestId?: string,
): Promise<NeteaseOperationResult<SourcePlaylist[]>> {
  const outcome = await dispatch(
    { action: "playlistList", source: NETEASE_SOURCE_ID, accountRef },
    requestId,
  );
  return operationResult(outcome, "playlistList", (response) => response.data);
}

export async function getNeteasePlaylist(
  accountRef: string,
  playlistId: string,
  requestId?: string,
): Promise<NeteaseOperationResult<SourcePlaylistDetail>> {
  const outcome = await dispatch(
    { action: "playlistRead", source: NETEASE_SOURCE_ID, accountRef, playlistId },
    requestId,
  );
  return operationResult(outcome, "playlistRead", (response) => response.data);
}

export async function addNeteasePlaylistTrack(
  accountRef: string,
  playlistId: string,
  track: RemoteTrack,
): Promise<NeteaseOperationResult<SourcePlaylistMutation>> {
  const outcome = await dispatch({
    action: "playlistAddTrack",
    source: NETEASE_SOURCE_ID,
    accountRef,
    playlistId,
    track: { id: track.id, source: track.source },
  });
  return operationResult(outcome, "playlistAddTrack", (response) => response.data);
}

export async function removeNeteasePlaylistTrack(
  accountRef: string,
  playlistId: string,
  track: RemoteTrack,
): Promise<NeteaseOperationResult<SourcePlaylistMutation>> {
  const outcome = await dispatch({
    action: "playlistRemoveTrack",
    source: NETEASE_SOURCE_ID,
    accountRef,
    playlistId,
    track: { id: track.id, source: track.source },
  });
  return operationResult(outcome, "playlistRemoveTrack", (response) => response.data);
}

export async function resolveNeteaseTrack(
  track: RemoteTrack,
  quality: SourceQuality,
  accountRef?: string,
  requestId?: string,
): Promise<NeteasePlayback> {
  const musicInfo = {
    ...track.rawInfo,
    id: track.id,
    ...(accountRef ? { accountRef } : {}),
  };
  const outcome = await dispatch(
    {
      action: "musicUrl",
      source: NETEASE_SOURCE_ID,
      musicInfo,
      quality,
    },
    requestId,
  );
  const result = operationResult(outcome, "musicUrl", (response) => response.data);
  return {
    track,
    url: result.data,
    mimeType: neteaseMediaType(result.data, quality),
    providerName: "NetEase Cloud Music",
    diagnostics: result.diagnostics,
  };
}

function neteaseMediaType(url: string, quality: SourceQuality) {
  const path = url.split("?", 1)[0].toLowerCase();
  if (path.endsWith(".m4a") || path.endsWith(".mp4")) {
    return "audio/mp4";
  }
  if (path.endsWith(".aac")) {
    return "audio/aac";
  }
  if (path.endsWith(".flac") || quality === "flac" || quality === "flac24bit") {
    return "audio/flac";
  }
  return "audio/mpeg";
}

function dispatch(request: SourceRequest, requestId?: string) {
  return dispatchPluginRequest(NETEASE_PLUGIN_ID, request, requestId);
}

function operationResult<Action extends SourceResponse["action"], T>(
  outcome: SourceRequestOutcome,
  action: Action,
  select: (response: Extract<SourceResponse, { action: Action }>) => T,
): NeteaseOperationResult<T> {
  if (outcome.response.action !== action) {
    throw new Error(`NetEase provider returned ${outcome.response.action} for ${action}`);
  }
  return {
    data: select(outcome.response as Extract<SourceResponse, { action: Action }>),
    diagnostics: outcome.diagnostics,
  };
}
