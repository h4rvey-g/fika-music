import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  KugouAccount,
  KugouQrLoginPoll,
  KugouQrLoginStart,
} from "../generated/bindings";
import {
  dispatchPluginRequest,
  type RemoteTrack,
  type SourceDiagnostic,
  type SourcePlaylist,
  type SourcePlaylistDetail,
  type SourceQuality,
  type SourceRequest,
  type SourceRequestOutcome,
  type SourceResponse,
} from "./plugin-api";

export const KUGOU_PLUGIN_ID = "fika.kugou";
export const KUGOU_SOURCE_ID = "kg";

export type { KugouAccount, KugouQrLoginPoll, KugouQrLoginStart } from "../generated/bindings";

export type KugouOperationResult<T> = {
  data: T;
  diagnostics: SourceDiagnostic[];
};

export type KugouPlayback = {
  track: RemoteTrack;
  url: string;
  providerName: string;
  diagnostics: SourceDiagnostic[];
};

export function startKugouQrLogin() {
  return invoke<KugouQrLoginStart>(TAURI_COMMANDS.startKugouQrLogin);
}

export function pollKugouQrLogin(sessionId: string) {
  return invoke<KugouQrLoginPoll>(TAURI_COMMANDS.pollKugouQrLogin, { sessionId });
}

export function cancelKugouQrLogin(sessionId: string) {
  return invoke<void>(TAURI_COMMANDS.cancelKugouQrLogin, { sessionId });
}

export function listKugouAccounts() {
  return invoke<KugouAccount[]>(TAURI_COMMANDS.listKugouAccounts);
}

export function disconnectKugouAccount(accountRef: string) {
  return invoke<void>(TAURI_COMMANDS.disconnectKugouAccount, { accountRef });
}

export async function getKugouRecommendations(
  accountRef: string,
  requestId?: string,
): Promise<KugouOperationResult<RemoteTrack[]>> {
  const outcome = await dispatch(
    {
      action: "musicRecommendations",
      source: KUGOU_SOURCE_ID,
      accountRef,
      limit: 50,
    },
    requestId,
  );
  return operationResult(outcome, "musicRecommendations", (response) => response.data.list);
}

export async function getKugouPlaylists(
  accountRef: string,
  requestId?: string,
): Promise<KugouOperationResult<SourcePlaylist[]>> {
  const outcome = await dispatch(
    { action: "playlistList", source: KUGOU_SOURCE_ID, accountRef },
    requestId,
  );
  return operationResult(outcome, "playlistList", (response) => response.data);
}

export async function getKugouPlaylist(
  accountRef: string,
  playlistId: string,
  requestId?: string,
): Promise<KugouOperationResult<SourcePlaylistDetail>> {
  const outcome = await dispatch(
    {
      action: "playlistRead",
      source: KUGOU_SOURCE_ID,
      accountRef,
      playlistId,
    },
    requestId,
  );
  return operationResult(outcome, "playlistRead", (response) => response.data);
}

export async function resolveKugouTrack(
  track: RemoteTrack,
  quality: SourceQuality,
  accountRef: string,
  requestId?: string,
): Promise<KugouPlayback> {
  const outcome = await dispatch(
    {
      action: "musicUrl",
      source: KUGOU_SOURCE_ID,
      musicInfo: {
        ...track.rawInfo,
        id: track.id,
        accountRef,
      },
      quality,
    },
    requestId,
  );
  const result = operationResult(outcome, "musicUrl", (response) => response.data);
  return {
    track,
    url: result.data,
    providerName: "KuGou Music",
    diagnostics: result.diagnostics,
  };
}

function dispatch(request: SourceRequest, requestId?: string) {
  return dispatchPluginRequest(KUGOU_PLUGIN_ID, request, requestId);
}

function operationResult<Action extends SourceResponse["action"], T>(
  outcome: SourceRequestOutcome,
  action: Action,
  select: (response: Extract<SourceResponse, { action: Action }>) => T,
): KugouOperationResult<T> {
  if (outcome.response.action !== action) {
    throw new Error(`KuGou provider returned ${outcome.response.action} for ${action}`);
  }
  return {
    data: select(outcome.response as Extract<SourceResponse, { action: Action }>),
    diagnostics: outcome.diagnostics,
  };
}
