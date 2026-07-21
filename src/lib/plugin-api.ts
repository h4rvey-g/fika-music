import { invoke } from "@tauri-apps/api/core";

export type PluginDiagnostic = {
  code: string;
  level: "info" | "warn" | "error" | "security" | string;
  sourceId: string | null;
  message: string;
  timestamp: number;
};

export type PluginProviderState = {
  id: string;
  entrypoint: string;
  initialized: boolean;
  sources: Array<{ id: string; name: string }>;
  diagnostics: PluginDiagnostic[];
};

export type PluginRecord = {
  id: string;
  name: string;
  version: string | null;
  description: string | null;
  author: string | null;
  path: string;
  origin: "bundled" | "user";
  state:
    | "disabled"
    | "needs-review"
    | "enabled"
    | "incompatible"
    | "error"
    | "invalid"
    | string;
  enabled: boolean;
  permissionsReviewed: boolean;
  declaredCapabilities: string[];
  grantedCapabilities: string[];
  requiredHostBridges: string[];
  providers: PluginProviderState[];
  diagnostics: PluginDiagnostic[];
  canRemove: boolean;
  canEnable: boolean;
};

export type SourceQuality = "128k" | "320k" | "flac" | "flac24bit";

export type SourceRequest =
  | {
      action: "musicSearch";
      source: string;
      keyword: string;
      page: number;
      pageSize: number;
    }
  | {
      action: "musicUrl";
      source: string;
      musicInfo: Record<string, unknown>;
      quality?: SourceQuality;
    }
  | {
      action: "lyric";
      source: string;
      musicInfo: Record<string, unknown>;
    }
  | {
      action: "pic";
      source: string;
      musicInfo: Record<string, unknown>;
    };

export type SourceDiagnostic = {
  sourceId: string;
  level: "info" | "warn" | "error" | "security";
  message: string;
};

export type SourceSearchResult = {
  id: string;
  source: string;
  title: string;
  artist: string;
  album: string | null;
  durationSeconds: number | null;
  coverUrl: string | null;
  rawInfo: Record<string, unknown>;
};

export type SourceResponse =
  | {
      action: "musicSearch";
      data: {
        isEnd: boolean;
        total: number | null;
        list: SourceSearchResult[];
      };
    }
  | { action: "musicUrl"; data: string }
  | {
      action: "lyric";
      data: {
        lyric: string | null;
        tlyric: string | null;
        rlyric: string | null;
        lxlyric: string | null;
      };
    }
  | { action: "pic"; data: string };

export type SourceRequestOutcome = {
  response: SourceResponse;
  diagnostics: SourceDiagnostic[];
};

export function listPlugins() {
  return invoke<PluginRecord[]>("list_plugins");
}

export function selectPluginPackage() {
  return invoke<string | null>("select_plugin_package");
}

export function refreshPluginRegistry() {
  return invoke<PluginRecord[]>("refresh_plugins");
}

export function installPluginPackage(packagePath: string) {
  return invoke<PluginRecord>("install_plugin_package", { packagePath });
}

export function setPluginCapabilities(
  pluginId: string,
  capabilities: string[],
  reviewed: boolean,
) {
  return invoke<PluginRecord>("set_plugin_capabilities", {
    pluginId,
    capabilities,
    reviewed,
  });
}

export function setPluginEnabled(pluginId: string, enabled: boolean) {
  return invoke<PluginRecord>("set_plugin_enabled", { pluginId, enabled });
}

export function removePluginPackage(pluginId: string) {
  return invoke<PluginRecord[]>("remove_plugin", { pluginId });
}

export function clearPluginDiagnostics(pluginId: string) {
  return invoke<PluginRecord>("clear_plugin_diagnostics", { pluginId });
}

export function dispatchPluginRequest(
  pluginId: string,
  request: SourceRequest,
  requestId?: string,
) {
  return invoke<SourceRequestOutcome>("dispatch_plugin_request", {
    pluginId,
    request,
    requestId,
  });
}
