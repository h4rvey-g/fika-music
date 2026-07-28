<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import {
  AlertCircle,
  AudioLines,
  ChevronDown,
  CircleCheck,
  Link,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
  X,
} from "@lucide/vue";
import {
  clearAudioSourceDiagnostics,
  importAudioSource,
  importAudioSourceUrl,
  listAudioSources,
  refreshAudioSources,
  removeAudioSource,
  selectAudioSourceFile,
  setAudioSourceCapabilities,
  setAudioSourceEnabled,
  type AudioSourceDiagnostic,
  type AudioSourceRecord,
} from "../lib/audio-source-api";
import type { SourceCapability } from "../generated/bindings";
import { normalizeError } from "../lib/errors";

const emit = defineEmits<{
  sourcesChanged: [sources: AudioSourceRecord[]];
}>();

const audioSources = ref<AudioSourceRecord[]>([]);
const expandedSourceId = ref<string | null>(null);
const isLoading = ref(false);
const importMode = ref<"local" | "url" | null>(null);
const isUrlDialogOpen = ref(false);
const sourceUrl = ref("");
const sourceUrlError = ref<string | null>(null);
const sourceUrlInput = ref<HTMLInputElement | null>(null);
const busySourceId = ref<string | null>(null);
const sourceError = ref<string | null>(null);
const sourceNotice = ref<string | null>(null);

const hasAudioSources = computed(() => audioSources.value.length > 0);

onMounted(() => {
  void loadAudioSources();
});

async function loadAudioSources() {
  isLoading.value = true;
  sourceError.value = null;
  try {
    replaceAudioSources(await listAudioSources());
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function refreshSources() {
  isLoading.value = true;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    replaceAudioSources(await refreshAudioSources());
    sourceNotice.value = "Audio sources refreshed.";
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function importLocalSource() {
  importMode.value = "local";
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    const sourcePath = await selectAudioSourceFile();
    if (!sourcePath) {
      return;
    }
    finishImport(await importAudioSource(sourcePath));
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    importMode.value = null;
  }
}

async function openUrlDialog() {
  sourceUrlError.value = null;
  isUrlDialogOpen.value = true;
  await nextTick();
  sourceUrlInput.value?.focus();
}

function closeUrlDialog() {
  if (importMode.value === "url") {
    return;
  }
  isUrlDialogOpen.value = false;
  sourceUrlError.value = null;
}

async function importFromUrl() {
  const url = sourceUrl.value.trim();
  if (!url) {
    sourceUrlError.value = "Source URL is required.";
    return;
  }
  importMode.value = "url";
  sourceUrlError.value = null;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    const imported = await importAudioSourceUrl(url);
    finishImport(imported);
    sourceUrl.value = "";
    isUrlDialogOpen.value = false;
  } catch (error) {
    sourceUrlError.value = normalizeError(error);
  } finally {
    importMode.value = null;
  }
}

function finishImport(imported: AudioSourceRecord) {
  replaceAudioSource(imported);
  expandedSourceId.value = imported.id;
  sourceNotice.value = `${imported.name} imported. Review its permissions before enabling it.`;
}

async function toggleEnabled(audioSource: AudioSourceRecord) {
  busySourceId.value = audioSource.id;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    replaceAudioSource(await setAudioSourceEnabled(audioSource.id, !audioSource.enabled));
  } catch (error) {
    sourceError.value = normalizeError(error);
    await loadAudioSources();
  } finally {
    busySourceId.value = null;
  }
}

async function updateCapability(
  audioSource: AudioSourceRecord,
  capability: SourceCapability,
  granted: boolean,
) {
  const nextCapabilities = new Set(audioSource.grantedCapabilities);
  if (granted) {
    nextCapabilities.add(capability);
  } else {
    nextCapabilities.delete(capability);
  }
  await saveCapabilities(audioSource, [...nextCapabilities], audioSource.permissionsReviewed);
}

async function reviewCapabilities(audioSource: AudioSourceRecord) {
  await saveCapabilities(audioSource, audioSource.grantedCapabilities, true);
}

async function saveCapabilities(
  audioSource: AudioSourceRecord,
  capabilities: SourceCapability[],
  reviewed: boolean,
) {
  busySourceId.value = audioSource.id;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    replaceAudioSource(
      await setAudioSourceCapabilities(audioSource.id, capabilities, reviewed),
    );
    sourceNotice.value = reviewed
      ? "Audio source permissions saved."
      : "Permission review is still required.";
  } catch (error) {
    sourceError.value = normalizeError(error);
    await loadAudioSources();
  } finally {
    busySourceId.value = null;
  }
}

async function removeSource(audioSource: AudioSourceRecord) {
  if (!window.confirm(`Remove ${audioSource.name}?`)) {
    return;
  }
  busySourceId.value = audioSource.id;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    replaceAudioSources(await removeAudioSource(audioSource.id));
    if (expandedSourceId.value === audioSource.id) {
      expandedSourceId.value = null;
    }
    sourceNotice.value = `${audioSource.name} removed.`;
  } catch (error) {
    sourceError.value = normalizeError(error);
    await loadAudioSources();
  } finally {
    busySourceId.value = null;
  }
}

async function clearDiagnostics(audioSource: AudioSourceRecord) {
  busySourceId.value = audioSource.id;
  sourceError.value = null;
  try {
    replaceAudioSource(await clearAudioSourceDiagnostics(audioSource.id));
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    busySourceId.value = null;
  }
}

function replaceAudioSource(updated: AudioSourceRecord) {
  const index = audioSources.value.findIndex((source) => source.id === updated.id);
  replaceAudioSources(
    index === -1
      ? [...audioSources.value, updated]
      : audioSources.value.map((source, sourceIndex) =>
          sourceIndex === index ? updated : source,
        ),
  );
}

function replaceAudioSources(updated: AudioSourceRecord[]) {
  audioSources.value = updated;
  emit("sourcesChanged", [...updated]);
}

function toggleDetails(audioSourceId: string) {
  expandedSourceId.value =
    expandedSourceId.value === audioSourceId ? null : audioSourceId;
}

function capabilityLabel(capability: string) {
  const labels: Record<string, string> = {
    "network:any": "Any network host",
    "account:ref": "Account references",
    "playlist:read": "Read playlists",
    "playlist:write": "Change playlists",
    "metadata:read": "Read metadata",
    "cache:read-write": "Read and write cache",
  };
  return labels[capability] || capability;
}

function stateLabel(state: AudioSourceRecord["state"]) {
  const labels: Record<AudioSourceRecord["state"], string> = {
    disabled: "Disabled",
    "needs-review": "Review required",
    enabled: "Enabled",
    incompatible: "Incompatible",
    error: "Load error",
    invalid: "Invalid manifest",
  };
  return labels[state];
}

function stateClass(state: AudioSourceRecord["state"]) {
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

function diagnosticClass(level: AudioSourceDiagnostic["level"]) {
  if (level === "security" || level === "error") {
    return "text-error";
  }
  if (level === "warn") {
    return "text-warning";
  }
  return "text-muted";
}

function formatTimestamp(timestamp: number) {
  return timestamp ? new Date(timestamp * 1000).toLocaleString() : "-";
}

</script>

<template>
  <section class="overflow-hidden border border-base-300 bg-base-100">
    <header class="flex flex-col gap-3 border-b border-base-300 p-4 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex min-w-0 items-center gap-3">
        <div class="flex size-10 shrink-0 items-center justify-center rounded bg-base-200">
          <AudioLines :size="19" aria-hidden="true" />
        </div>
        <div class="min-w-0">
          <h2 class="text-base font-semibold">Audio Sources</h2>
          <p class="mt-0.5 text-xs text-muted">
            {{ audioSources.length }} imported source{{ audioSources.length === 1 ? "" : "s" }}
          </p>
        </div>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="isLoading || importMode !== null"
          aria-label="Refresh audio sources"
          title="Refresh"
          @click="refreshSources"
        >
          <RefreshCw :class="{ 'animate-spin': isLoading }" :size="16" aria-hidden="true" />
        </button>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="importMode !== null"
          @click="openUrlDialog"
        >
          <Link :size="16" aria-hidden="true" />
          Import URL
        </button>
        <button
          class="btn btn-primary btn-sm"
          type="button"
          :disabled="importMode !== null"
          @click="importLocalSource"
        >
          <RefreshCw
            v-if="importMode === 'local'"
            class="animate-spin"
            :size="16"
            aria-hidden="true"
          />
          <Upload v-else :size="16" aria-hidden="true" />
          Import file
        </button>
      </div>
    </header>

    <dialog
      v-if="isUrlDialogOpen"
      open
      class="modal"
      aria-labelledby="audio-source-url-title"
      @cancel.prevent="closeUrlDialog"
    >
      <form class="modal-box max-w-xl" @submit.prevent="importFromUrl">
        <div class="flex items-center justify-between gap-3">
          <h3 id="audio-source-url-title" class="text-base font-semibold">
            Import audio source
          </h3>
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :disabled="importMode === 'url'"
            aria-label="Close URL import"
            @click="closeUrlDialog"
          >
            <X :size="17" aria-hidden="true" />
          </button>
        </div>
        <fieldset class="fieldset mt-4">
          <legend class="fieldset-legend">Source URL</legend>
          <input
            ref="sourceUrlInput"
            v-model="sourceUrl"
            class="input w-full"
            type="url"
            inputmode="url"
            autocomplete="url"
            spellcheck="false"
            placeholder="https://example.com/source.js"
            :disabled="importMode === 'url'"
            required
          />
        </fieldset>
        <div v-if="sourceUrlError" role="alert" class="alert alert-error alert-soft mt-3">
          <AlertCircle :size="18" aria-hidden="true" />
          <span>{{ sourceUrlError }}</span>
        </div>
        <div class="modal-action">
          <button
            class="btn"
            type="button"
            :disabled="importMode === 'url'"
            @click="closeUrlDialog"
          >
            Cancel
          </button>
          <button class="btn btn-primary" type="submit" :disabled="importMode === 'url'">
            <RefreshCw
              v-if="importMode === 'url'"
              class="animate-spin"
              :size="16"
              aria-hidden="true"
            />
            <Link v-else :size="16" aria-hidden="true" />
            Import
          </button>
        </div>
      </form>
      <form method="dialog" class="modal-backdrop" @submit.prevent="closeUrlDialog">
        <button type="submit" :disabled="importMode === 'url'">Close</button>
      </form>
    </dialog>

    <div v-if="sourceError" role="alert" class="alert alert-error m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceError }}</span>
      <button
        class="btn btn-square btn-ghost btn-sm"
        type="button"
        aria-label="Dismiss error"
        @click="sourceError = null"
      >
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="sourceNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceNotice }}</span>
      <button
        class="btn btn-square btn-ghost btn-sm"
        type="button"
        aria-label="Dismiss notice"
        @click="sourceNotice = null"
      >
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="isLoading && !hasAudioSources" class="flex items-center gap-2 p-6 text-sm text-muted">
      <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
      Loading audio sources
    </div>

    <div v-else-if="!hasAudioSources" class="p-8 text-center">
      <AudioLines class="mx-auto text-base-content/35" :size="30" aria-hidden="true" />
      <p class="mt-3 text-sm font-medium">No audio sources imported</p>
    </div>

    <ul v-else class="list divide-y divide-base-300">
      <li
        v-for="audioSource in audioSources"
        :key="audioSource.id"
        class="grid grid-cols-[2.5rem_minmax(0,1fr)_auto] gap-3 px-4 py-4"
      >
        <div class="col-start-1 row-start-1 flex size-10 shrink-0 items-center justify-center rounded bg-base-200">
          <AudioLines :size="19" aria-hidden="true" />
        </div>

        <div
          class="min-w-0"
          :class="
            expandedSourceId === audioSource.id
              ? 'col-span-3 row-start-2 sm:col-span-1 sm:col-start-2 sm:row-start-1'
              : 'col-start-2 row-start-1'
          "
        >
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="font-medium">{{ audioSource.name }}</h3>
            <span class="badge badge-sm" :class="stateClass(audioSource.state)">
              {{ stateLabel(audioSource.state) }}
            </span>
            <span v-if="audioSource.adapter" class="badge badge-outline badge-sm">
              {{ audioSource.adapter }}
            </span>
          </div>
          <p class="mt-1 truncate text-xs text-muted">
            {{ audioSource.id }}<span v-if="audioSource.version"> / v{{ audioSource.version }}</span>
          </p>
          <p v-if="audioSource.description" class="mt-2 text-sm text-muted">
            {{ audioSource.description }}
          </p>

          <div
            v-if="expandedSourceId === audioSource.id"
            class="mt-4 space-y-4 border-t border-base-300 pt-4"
          >
            <div class="grid gap-2 text-xs text-muted sm:grid-cols-2">
              <span>{{ audioSource.sources.length }} catalog source{{ audioSource.sources.length === 1 ? "" : "s" }}</span>
              <span class="truncate" :title="audioSource.path">{{ audioSource.path }}</span>
            </div>

            <div v-if="audioSource.declaredCapabilities.length" class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">Permissions</h4>
                <span
                  v-if="audioSource.permissionsReviewed"
                  class="flex items-center gap-1 text-xs text-success"
                >
                  <ShieldCheck :size="14" aria-hidden="true" />
                  Reviewed
                </span>
                <span v-else class="text-xs text-warning">Review required</span>
              </div>
              <div class="grid gap-2 sm:grid-cols-2">
                <label
                  v-for="capability in audioSource.declaredCapabilities"
                  :key="capability"
                  class="flex min-h-10 items-center justify-between gap-3 border border-base-300 px-3 py-2 text-sm"
                >
                  <span>{{ capabilityLabel(capability) }}</span>
                  <input
                class="checkbox checkbox-md"
                    type="checkbox"
                    :checked="audioSource.grantedCapabilities.includes(capability)"
                    :disabled="busySourceId === audioSource.id"
                    :aria-label="`Grant ${capabilityLabel(capability)}`"
                    @change="updateCapability(audioSource, capability, ($event.target as HTMLInputElement).checked)"
                  />
                </label>
              </div>
              <div
                v-if="!audioSource.permissionsReviewed"
                class="alert alert-warning alert-soft alert-vertical sm:alert-horizontal"
              >
                <AlertCircle :size="17" aria-hidden="true" />
                <span class="text-sm">
                  Confirm before enabling. This imported JavaScript can contact any network host through Fika.
                </span>
                <button
                  class="btn btn-sm"
                  type="button"
                  :disabled="busySourceId === audioSource.id"
                  @click="reviewCapabilities(audioSource)"
                >
              <ShieldCheck :size="16" aria-hidden="true" />
                  Confirm review
                </button>
              </div>
            </div>

            <div class="space-y-2">
              <h4 class="text-sm font-semibold">Catalog</h4>
              <div class="flex flex-wrap gap-2">
                <span
                  v-for="source in audioSource.sources"
                  :key="source.id"
                  class="badge badge-ghost"
                >
                  {{ source.name }} / {{ source.id }}
                </span>
              </div>
            </div>

            <div class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">Diagnostics</h4>
                <button
                  v-if="audioSource.diagnostics.length"
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :disabled="busySourceId === audioSource.id"
                  @click="clearDiagnostics(audioSource)"
                >
                  Clear
                </button>
              </div>
              <p v-if="!audioSource.diagnostics.length" class="text-xs text-muted">
                No diagnostics.
              </p>
              <ul v-else class="max-h-52 space-y-2 overflow-y-auto border border-base-300 p-3">
                <li
                  v-for="(diagnostic, index) in audioSource.diagnostics"
                  :key="`${diagnostic.timestamp}-${index}`"
                  class="text-xs"
                >
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-medium uppercase" :class="diagnosticClass(diagnostic.level)">
                      {{ diagnostic.level }}
                    </span>
                    <span class="text-muted">{{ diagnostic.code }}</span>
                    <span class="text-muted">{{ formatTimestamp(diagnostic.timestamp) }}</span>
                  </div>
                  <p class="mt-1 break-words text-muted">{{ diagnostic.message }}</p>
                </li>
              </ul>
            </div>
          </div>
        </div>

        <div class="col-start-3 row-start-1 flex shrink-0 items-start gap-2">
          <label class="flex items-center gap-2 text-xs">
            <span class="sr-only">Enable {{ audioSource.name }}</span>
            <input
            class="toggle toggle-md"
              type="checkbox"
              :checked="audioSource.enabled"
              :disabled="busySourceId === audioSource.id || (!audioSource.enabled && !audioSource.canEnable)"
              :aria-label="`Enable ${audioSource.name}`"
              @change="toggleEnabled(audioSource)"
            />
          </label>
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :aria-label="expandedSourceId === audioSource.id ? `Collapse ${audioSource.name}` : `Inspect ${audioSource.name}`"
            :title="expandedSourceId === audioSource.id ? 'Collapse details' : 'Inspect details'"
            @click="toggleDetails(audioSource.id)"
          >
            <ChevronDown
              :class="{ 'rotate-180': expandedSourceId === audioSource.id }"
              :size="17"
              aria-hidden="true"
            />
          </button>
          <button
            v-if="audioSource.canRemove"
            class="btn btn-square btn-ghost btn-sm text-error"
            type="button"
            :disabled="busySourceId === audioSource.id"
            :aria-label="`Remove ${audioSource.name}`"
            title="Remove audio source"
            @click="removeSource(audioSource)"
          >
            <Trash2 :size="16" aria-hidden="true" />
          </button>
        </div>
      </li>
    </ul>
  </section>
</template>
