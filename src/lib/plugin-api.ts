import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  PluginRecord,
  SourceCapability,
  SourceRequest,
  SourceRequestOutcome,
  SourceSearchResult,
} from "../generated/bindings";

export type {
  PluginDiagnostic,
  PluginProviderState,
  PluginRecord,
  SourceCapability,
  SourceDiagnostic,
  SourcePlaylist,
  SourcePlaylistDetail,
  SourcePlaylistMutation,
  SourceQuality,
  SourceRequest,
  SourceRequestOutcome,
  SourceResponse,
  SourceSearchResult,
  SourceTrackRef,
} from "../generated/bindings";

export type RemoteTrack = SourceSearchResult;

export function listPlugins() {
  return invoke<PluginRecord[]>(TAURI_COMMANDS.listPlugins);
}

export function selectPluginPackage() {
  return invoke<string | null>(TAURI_COMMANDS.selectPluginPackage);
}

export function refreshPluginRegistry() {
  return invoke<PluginRecord[]>(TAURI_COMMANDS.refreshPlugins);
}

export function installPluginPackage(packagePath: string) {
  return invoke<PluginRecord>(TAURI_COMMANDS.installPluginPackage, { packagePath });
}

export function setPluginCapabilities(
  pluginId: string,
  capabilities: SourceCapability[],
  reviewed: boolean,
) {
  return invoke<PluginRecord>(TAURI_COMMANDS.setPluginCapabilities, {
    pluginId,
    capabilities,
    reviewed,
  });
}

export function setPluginEnabled(pluginId: string, enabled: boolean) {
  return invoke<PluginRecord>(TAURI_COMMANDS.setPluginEnabled, { pluginId, enabled });
}

export function removePluginPackage(pluginId: string) {
  return invoke<PluginRecord[]>(TAURI_COMMANDS.removePlugin, { pluginId });
}

export function clearPluginDiagnostics(pluginId: string) {
  return invoke<PluginRecord>(TAURI_COMMANDS.clearPluginDiagnostics, { pluginId });
}

export function dispatchPluginRequest(
  pluginId: string,
  request: SourceRequest,
  requestId?: string,
) {
  return invoke<SourceRequestOutcome>(TAURI_COMMANDS.dispatchPluginRequest, {
    pluginId,
    request,
    requestId,
  });
}

export function cancelSourceRequest(requestId: string) {
  return invoke<boolean>(TAURI_COMMANDS.cancelSourceRequest, { requestId });
}
