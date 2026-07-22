<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  AlertCircle,
  ChevronDown,
  CircleCheck,
  Plug,
  Power,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
  X,
} from "@lucide/vue";
import {
  clearPluginDiagnostics,
  installPluginPackage,
  listPlugins,
  refreshPluginRegistry,
  removePluginPackage,
  selectPluginPackage,
  setPluginCapabilities,
  setPluginEnabled,
} from "../lib/plugin-api";
import type { PluginDiagnostic, PluginRecord, SourceCapability } from "../lib/plugin-api";

const emit = defineEmits<{
  pluginsChanged: [plugins: PluginRecord[]];
}>();

const plugins = ref<PluginRecord[]>([]);
const expandedPluginId = ref<string | null>(null);
const isLoading = ref(false);
const isInstalling = ref(false);
const busyPluginId = ref<string | null>(null);
const pluginError = ref<string | null>(null);
const pluginNotice = ref<string | null>(null);

const hasPlugins = computed(() => plugins.value.length > 0);

onMounted(() => {
  void loadPlugins();
});

async function loadPlugins() {
  isLoading.value = true;
  pluginError.value = null;

  try {
    replacePlugins(await listPlugins());
  } catch (error) {
    pluginError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function refreshPlugins() {
  isLoading.value = true;
  pluginError.value = null;
  pluginNotice.value = null;

  try {
    replacePlugins(await refreshPluginRegistry());
    pluginNotice.value = "Plugin registry refreshed.";
  } catch (error) {
    pluginError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function installPlugin() {
  isInstalling.value = true;
  pluginError.value = null;
  pluginNotice.value = null;

  try {
    const packagePath = await selectPluginPackage();
    if (!packagePath) {
      return;
    }
    const installed = await installPluginPackage(packagePath);
    replacePlugin(installed);
    expandedPluginId.value = installed.id;
    pluginNotice.value =
      installed.state === "needs-review"
        ? `${installed.name} installed and is awaiting permission review.`
        : `${installed.name} installed.`;
  } catch (error) {
    pluginError.value = normalizeError(error);
  } finally {
    isInstalling.value = false;
  }
}

async function toggleEnabled(plugin: PluginRecord) {
  busyPluginId.value = plugin.id;
  pluginError.value = null;
  pluginNotice.value = null;

  try {
    const updated = await setPluginEnabled(plugin.id, !plugin.enabled);
    replacePlugin(updated);
  } catch (error) {
    pluginError.value = normalizeError(error);
    await loadPlugins();
  } finally {
    busyPluginId.value = null;
  }
}

async function updateCapability(
  plugin: PluginRecord,
  capability: SourceCapability,
  granted: boolean,
) {
  const nextCapabilities = new Set(plugin.grantedCapabilities);
  if (granted) {
    nextCapabilities.add(capability);
  } else {
    nextCapabilities.delete(capability);
  }
  await saveCapabilities(plugin, [...nextCapabilities], plugin.permissionsReviewed);
}

async function reviewCapabilities(plugin: PluginRecord) {
  await saveCapabilities(plugin, plugin.grantedCapabilities, true);
}

async function saveCapabilities(
  plugin: PluginRecord,
  capabilities: SourceCapability[],
  reviewed: boolean,
) {
  busyPluginId.value = plugin.id;
  pluginError.value = null;
  pluginNotice.value = null;

  try {
    const updated = await setPluginCapabilities(plugin.id, capabilities, reviewed);
    replacePlugin(updated);
    pluginNotice.value = reviewed
      ? "Plugin permissions saved."
      : "Plugin permissions changed; review is still required.";
  } catch (error) {
    pluginError.value = normalizeError(error);
    await loadPlugins();
  } finally {
    busyPluginId.value = null;
  }
}

async function removePlugin(plugin: PluginRecord) {
  if (!window.confirm(`Remove ${plugin.name}?`)) {
    return;
  }

  busyPluginId.value = plugin.id;
  pluginError.value = null;
  pluginNotice.value = null;

  try {
    replacePlugins(await removePluginPackage(plugin.id));
    if (expandedPluginId.value === plugin.id) {
      expandedPluginId.value = null;
    }
    pluginNotice.value = `${plugin.name} removed.`;
  } catch (error) {
    pluginError.value = normalizeError(error);
    await loadPlugins();
  } finally {
    busyPluginId.value = null;
  }
}

async function clearDiagnostics(plugin: PluginRecord) {
  busyPluginId.value = plugin.id;
  pluginError.value = null;

  try {
    const updated = await clearPluginDiagnostics(plugin.id);
    replacePlugin(updated);
  } catch (error) {
    pluginError.value = normalizeError(error);
  } finally {
    busyPluginId.value = null;
  }
}

function replacePlugin(updated: PluginRecord) {
  const index = plugins.value.findIndex((plugin) => plugin.id === updated.id);
  if (index === -1) {
    replacePlugins([...plugins.value, updated]);
    return;
  }
  replacePlugins(
    plugins.value.map((plugin, pluginIndex) =>
      pluginIndex === index ? updated : plugin,
    ),
  );
}

function replacePlugins(updated: PluginRecord[]) {
  plugins.value = updated;
  emit("pluginsChanged", [...updated]);
}

function toggleDetails(pluginId: string) {
  expandedPluginId.value = expandedPluginId.value === pluginId ? null : pluginId;
}

function capabilityLabel(capability: string) {
  const labels: Record<string, string> = {
    "network:any": "Network requests",
    "account:ref": "Account references",
    "playlist:read": "Read playlists",
    "playlist:write": "Change playlists",
    "metadata:read": "Read metadata",
    "cache:read-write": "Read and write cache",
    "bridge:netease-api-enhanced": "NetEase service bridge",
  };
  return labels[capability] || capability;
}

function stateLabel(state: PluginRecord["state"]) {
  const labels: Record<string, string> = {
    disabled: "Disabled",
    "needs-review": "Review required",
    enabled: "Enabled",
    incompatible: "Incompatible",
    error: "Load error",
    invalid: "Invalid manifest",
  };
  return labels[state] || state;
}

function stateClass(state: PluginRecord["state"]) {
  if (state === "enabled") {
    return "badge-success";
  }
  if (state === "needs-review") {
    return "badge-warning";
  }
  if (state === "incompatible" || state === "error" || state === "invalid") {
    return "badge-error";
  }
  return "badge-ghost";
}

function diagnosticClass(level: PluginDiagnostic["level"]) {
  if (level === "security") {
    return "text-error";
  }
  if (level === "error") {
    return "text-error";
  }
  if (level === "warn") {
    return "text-warning";
  }
  return "text-base-content/70";
}

function formatTimestamp(timestamp: number) {
  if (!timestamp) {
    return "-";
  }
  return new Date(timestamp * 1000).toLocaleString();
}

function sourceCount(plugin: PluginRecord) {
  return plugin.providers.reduce((count, provider) => count + provider.sources.length, 0);
}

function normalizeError(error: unknown) {
  if (typeof error === "string") {
    try {
      const parsed = JSON.parse(error) as { message?: unknown };
      if (typeof parsed.message === "string") {
        return parsed.message;
      }
    } catch {
      return error;
    }
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") {
      return message;
    }
  }
  return "Unexpected Plugin System error.";
}
</script>

<template>
  <section class="rounded border border-base-300 bg-base-100">
    <div class="flex flex-col gap-3 border-b border-base-300 p-4 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <h2 class="flex items-center gap-2 text-base font-semibold">
          <Plug :size="18" aria-hidden="true" />
          Plugin System
        </h2>
        <p class="mt-1 text-sm text-base-content/65">
          {{ plugins.length }} installed package{{ plugins.length === 1 ? "" : "s" }}
        </p>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-sm"
          type="button"
          :disabled="isLoading || isInstalling"
          title="Refresh Plugin registry"
          @click="refreshPlugins"
        >
          <RefreshCw :class="{ 'animate-spin': isLoading }" :size="16" aria-hidden="true" />
          Refresh
        </button>
        <button
          class="btn btn-primary btn-sm"
          type="button"
          :disabled="isInstalling"
          @click="installPlugin"
        >
          <Upload :size="16" aria-hidden="true" />
          Install package
        </button>
      </div>
    </div>

    <div v-if="pluginError" role="alert" class="alert alert-error m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span>{{ pluginError }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss error" @click="pluginError = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="pluginNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span>{{ pluginNotice }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss notice" @click="pluginNotice = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="isLoading && !hasPlugins" class="flex items-center gap-2 p-6 text-sm text-base-content/65">
      <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
      Loading Plugins
    </div>

    <div v-else-if="!hasPlugins" class="p-8 text-center text-sm text-base-content/65">
      No Plugin packages discovered.
    </div>

    <ul v-else class="list divide-y divide-base-300">
      <li v-for="plugin in plugins" :key="plugin.id" class="list-row list-col-wrap gap-3 px-4 py-4">
        <div class="flex size-10 shrink-0 items-center justify-center rounded bg-base-200">
          <Plug :size="19" aria-hidden="true" />
        </div>

        <div class="list-col-grow min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="font-medium">{{ plugin.name }}</h3>
            <span class="badge badge-sm" :class="stateClass(plugin.state)">{{ stateLabel(plugin.state) }}</span>
            <span class="badge badge-outline badge-sm">{{ plugin.origin }}</span>
          </div>
          <p class="mt-1 truncate text-xs text-base-content/60">
            {{ plugin.id }}<span v-if="plugin.version"> / v{{ plugin.version }}</span>
          </p>
          <p v-if="plugin.description" class="mt-2 text-sm text-base-content/70">{{ plugin.description }}</p>

          <div v-if="expandedPluginId === plugin.id" class="mt-4 space-y-4 border-t border-base-300 pt-4">
            <div class="grid gap-2 text-xs text-base-content/65 sm:grid-cols-3">
              <span>{{ plugin.providers.length }} provider{{ plugin.providers.length === 1 ? "" : "s" }}</span>
              <span>{{ sourceCount(plugin) }} source{{ sourceCount(plugin) === 1 ? "" : "s" }}</span>
              <span class="truncate" :title="plugin.path">{{ plugin.path }}</span>
            </div>

            <div v-if="plugin.declaredCapabilities.length" class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">Capabilities</h4>
                <span v-if="plugin.permissionsReviewed" class="flex items-center gap-1 text-xs text-success">
                  <ShieldCheck :size="14" aria-hidden="true" />
                  Reviewed
                </span>
                <span v-else class="text-xs text-warning">Review required</span>
              </div>
              <div class="grid gap-2 sm:grid-cols-2">
                <label
                  v-for="capability in plugin.declaredCapabilities"
                  :key="capability"
                  class="flex min-h-10 items-center justify-between gap-3 rounded border border-base-300 px-3 py-2 text-sm"
                >
                  <span>{{ capabilityLabel(capability) }}</span>
                  <input
                    class="toggle toggle-sm"
                    type="checkbox"
                    :checked="plugin.grantedCapabilities.includes(capability)"
                    :disabled="busyPluginId === plugin.id"
                    :aria-label="`Grant ${capabilityLabel(capability)}`"
                    @change="updateCapability(plugin, capability, ($event.target as HTMLInputElement).checked)"
                  />
                </label>
              </div>
              <div v-if="!plugin.permissionsReviewed" class="alert alert-warning alert-soft">
                <AlertCircle :size="17" aria-hidden="true" />
                <span class="text-sm">Review the selected capabilities before enabling this Plugin.</span>
                <button
                  class="btn btn-sm"
                  type="button"
                  :disabled="busyPluginId === plugin.id"
                  @click="reviewCapabilities(plugin)"
                >
                  <ShieldCheck :size="15" aria-hidden="true" />
                  Confirm review
                </button>
              </div>
            </div>

            <div v-if="plugin.requiredHostBridges.length" class="space-y-2">
              <h4 class="text-sm font-semibold">Required host bridges</h4>
              <div class="flex flex-wrap gap-2">
                <span v-for="bridge in plugin.requiredHostBridges" :key="bridge" class="badge badge-outline">
                  {{ bridge }}
                </span>
              </div>
            </div>

            <div class="space-y-2">
              <h4 class="text-sm font-semibold">Source Providers</h4>
              <div v-for="provider in plugin.providers" :key="provider.id" class="rounded border border-base-300 p-3">
                <div class="flex flex-wrap items-center justify-between gap-2">
                  <span class="text-sm font-medium">{{ provider.id }}</span>
                  <span class="text-xs" :class="provider.initialized ? 'text-success' : 'text-base-content/60'">
                    {{ provider.initialized ? "Initialized" : "Not initialized" }}
                  </span>
                </div>
                <div class="mt-1 text-xs text-base-content/60">{{ provider.entrypoint }}</div>
                <div v-if="provider.sources.length" class="mt-2 flex flex-wrap gap-1">
                  <span v-for="source in provider.sources" :key="source.id" class="badge badge-ghost badge-sm">
                    {{ source.id }} / {{ source.name }}
                  </span>
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">Diagnostics</h4>
                <button
                  v-if="plugin.diagnostics.length"
                  class="btn btn-ghost btn-xs"
                  type="button"
                  :disabled="busyPluginId === plugin.id"
                  @click="clearDiagnostics(plugin)"
                >
                  Clear
                </button>
              </div>
              <div v-if="!plugin.diagnostics.length" class="text-xs text-base-content/60">No diagnostics.</div>
              <ul v-else class="max-h-52 space-y-2 overflow-y-auto rounded border border-base-300 p-3">
                <li v-for="(diagnostic, index) in plugin.diagnostics" :key="`${diagnostic.timestamp}-${index}`" class="text-xs">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-medium uppercase" :class="diagnosticClass(diagnostic.level)">{{ diagnostic.level }}</span>
                    <span class="text-base-content/50">{{ diagnostic.code }}</span>
                    <span class="text-base-content/50">{{ formatTimestamp(diagnostic.timestamp) }}</span>
                  </div>
                  <p class="mt-1 break-words text-base-content/75">{{ diagnostic.message }}</p>
                </li>
              </ul>
            </div>
          </div>
        </div>

        <div class="flex shrink-0 items-start gap-1">
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :aria-label="expandedPluginId === plugin.id ? `Collapse ${plugin.name}` : `Inspect ${plugin.name}`"
            :title="expandedPluginId === plugin.id ? 'Collapse details' : 'Inspect details'"
            @click="toggleDetails(plugin.id)"
          >
            <ChevronDown :class="{ 'rotate-180': expandedPluginId === plugin.id }" :size="17" aria-hidden="true" />
          </button>
          <button
            v-if="plugin.canEnable || plugin.enabled"
            class="btn btn-sm"
            :class="plugin.enabled ? 'btn-ghost' : 'btn-primary'"
            type="button"
            :disabled="busyPluginId === plugin.id || (!plugin.enabled && !plugin.canEnable)"
            @click="toggleEnabled(plugin)"
          >
            <Power :size="15" aria-hidden="true" />
            {{ plugin.enabled ? "Disable" : "Enable" }}
          </button>
          <button
            v-if="plugin.canRemove"
            class="btn btn-square btn-ghost btn-sm text-error"
            type="button"
            :disabled="busyPluginId === plugin.id"
            :aria-label="`Remove ${plugin.name}`"
            title="Remove Plugin"
            @click="removePlugin(plugin)"
          >
            <Trash2 :size="16" aria-hidden="true" />
          </button>
        </div>
      </li>
    </ul>
  </section>
</template>
