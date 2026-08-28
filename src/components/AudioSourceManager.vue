<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import {
  AlertCircle,
  AudioLines,
  ChevronDown,
  CircleCheck,
  CircleX,
  Link,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
  X,
} from "@lucide/vue";
import {
  checkAudioSourceAvailability,
  clearAudioSourceDiagnostics,
  importAudioSource,
  importAudioSourceUrl,
  listAudioSources,
  refreshAudioSources,
  removeAudioSource,
  selectAudioSourceFile,
  setAudioSourceCapabilities,
  setAudioSourceEnabled,
  type AudioSourceAvailability,
  type AudioSourceDiagnostic,
  type AudioSourceRecord,
} from "../lib/audio-source-api";
import { CHKSZ_AUDIO_SOURCE_ID } from "../lib/chksz-audio-source-api";
import type { SourceCapability } from "../generated/bindings";
import { normalizeError } from "../lib/errors";
import { currentLocale, t } from "../i18n";
import ChkszAudioSourceSettings from "./ChkszAudioSourceSettings.vue";

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
const sourceToRemove = ref<AudioSourceRecord | null>(null);
const removeConfirmButton = ref<HTMLButtonElement | null>(null);
const busySourceId = ref<string | null>(null);
const sourceError = ref<string | null>(null);
const sourceNotice = ref<string | null>(null);
const availabilityResults = ref<Record<string, AudioSourceAvailability>>({});
const availabilityCheck = ref<{ audioSourceId: string; sourceId: string | null } | null>(null);

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
    sourceNotice.value = t("Audio sources refreshed.");
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
    sourceUrlError.value = t("Source URL is required.");
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
  sourceNotice.value = t("{name} imported.", { name: imported.name });
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
      ? t("Audio source permissions saved.")
      : t("Permission review is still required.");
  } catch (error) {
    sourceError.value = normalizeError(error);
    await loadAudioSources();
  } finally {
    busySourceId.value = null;
  }
}

async function requestRemoveSource(audioSource: AudioSourceRecord) {
  sourceToRemove.value = audioSource;
  await nextTick();
  removeConfirmButton.value?.focus();
}

function closeRemoveDialog() {
  if (sourceToRemove.value?.id === busySourceId.value) {
    return;
  }
  sourceToRemove.value = null;
}

async function removeSource() {
  const audioSource = sourceToRemove.value;
  if (!audioSource) return;

  busySourceId.value = audioSource.id;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    replaceAudioSources(await removeAudioSource(audioSource.id));
    if (expandedSourceId.value === audioSource.id) {
      expandedSourceId.value = null;
    }
    sourceNotice.value = t("{name} removed.", { name: audioSource.name });
  } catch (error) {
    sourceError.value = normalizeError(error);
    await loadAudioSources();
  } finally {
    busySourceId.value = null;
    sourceToRemove.value = null;
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

async function checkAvailability(audioSource: AudioSourceRecord, sourceId?: string) {
  availabilityCheck.value = { audioSourceId: audioSource.id, sourceId: sourceId ?? null };
  sourceError.value = null;
  try {
    const results = await checkAudioSourceAvailability(audioSource.id, sourceId);
    const next = { ...availabilityResults.value };
    for (const result of results) {
      next[availabilityKey(result.audioSourceId, result.sourceId)] = result;
    }
    availabilityResults.value = next;
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    availabilityCheck.value = null;
  }
}

function availabilityKey(audioSourceId: string, sourceId: string) {
  return `${audioSourceId}::${sourceId}`;
}

function availabilityFor(audioSourceId: string, sourceId: string) {
  return availabilityResults.value[availabilityKey(audioSourceId, sourceId)] ?? null;
}

function availabilityClass(result: AudioSourceAvailability | null) {
  if (!result) return "text-muted";
  return result.available ? "text-success" : "text-error";
}

function isCheckingAvailability(audioSourceId: string, sourceId?: string) {
  return availabilityCheck.value?.audioSourceId === audioSourceId
    && (sourceId === undefined || availabilityCheck.value.sourceId === sourceId);
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
  return labels[capability] ? t(labels[capability]) : capability;
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
  return t(labels[state]);
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
  return timestamp ? new Date(timestamp * 1000).toLocaleString(currentLocale.value) : "-";
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
          <h2 class="text-base font-semibold">{{ t("Audio Sources") }}</h2>
          <p class="mt-0.5 text-xs text-muted">
            {{ t(audioSources.length === 1 ? "{count} audio source" : "{count} audio sources", { count: audioSources.length }) }}
          </p>
        </div>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="isLoading || importMode !== null"
          :aria-label="t('Refresh audio sources')"
          :title="t('Refresh')"
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
          {{ t("Import URL") }}
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
          {{ t("Import file") }}
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
            {{ t("Import audio source") }}
          </h3>
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :disabled="importMode === 'url'"
            :aria-label="t('Close URL import')"
            @click="closeUrlDialog"
          >
            <X :size="17" aria-hidden="true" />
          </button>
        </div>
        <fieldset class="fieldset mt-4">
          <legend class="fieldset-legend">{{ t("Source URL") }}</legend>
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
            {{ t("Cancel") }}
          </button>
          <button class="btn btn-primary" type="submit" :disabled="importMode === 'url'">
            <RefreshCw
              v-if="importMode === 'url'"
              class="animate-spin"
              :size="16"
              aria-hidden="true"
            />
            <Link v-else :size="16" aria-hidden="true" />
            {{ t("Import") }}
          </button>
        </div>
      </form>
      <form method="dialog" class="modal-backdrop" @submit.prevent="closeUrlDialog">
        <button type="submit" :disabled="importMode === 'url'">{{ t("Close") }}</button>
      </form>
    </dialog>

    <dialog
      v-if="sourceToRemove"
      open
      class="modal"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="audio-source-remove-title"
      aria-describedby="audio-source-remove-description"
      @cancel.prevent="closeRemoveDialog"
    >
      <div class="modal-box max-w-md">
        <h3 id="audio-source-remove-title" class="text-base font-semibold">
          {{ t("Remove audio source") }}
        </h3>
        <p id="audio-source-remove-description" class="mt-3 text-sm text-muted">
          {{ t("Remove {name}?", { name: sourceToRemove.name }) }}
        </p>
        <div class="modal-action">
          <button
            class="btn"
            type="button"
            :disabled="busySourceId === sourceToRemove.id"
            @click="closeRemoveDialog"
          >
            {{ t("Cancel") }}
          </button>
          <button
            ref="removeConfirmButton"
            class="btn btn-error"
            type="button"
            :disabled="busySourceId === sourceToRemove.id"
            :aria-label="t('Confirm remove {name}', { name: sourceToRemove.name })"
            @click="removeSource"
          >
            <RefreshCw
              v-if="busySourceId === sourceToRemove.id"
              class="animate-spin"
              :size="16"
              aria-hidden="true"
            />
            <Trash2 v-else :size="16" aria-hidden="true" />
            {{ t("Remove") }}
          </button>
        </div>
      </div>
      <form method="dialog" class="modal-backdrop" @submit.prevent="closeRemoveDialog">
        <button type="submit" :disabled="busySourceId === sourceToRemove.id">
          {{ t("Cancel") }}
        </button>
      </form>
    </dialog>

    <div v-if="sourceError" role="alert" class="alert alert-error m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceError }}</span>
      <button
        class="btn btn-square btn-ghost btn-sm"
        type="button"
        :aria-label="t('Dismiss error')"
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
        :aria-label="t('Dismiss notice')"
        @click="sourceNotice = null"
      >
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="isLoading && !hasAudioSources" class="flex items-center gap-2 p-6 text-sm text-muted">
      <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
      {{ t("Loading audio sources") }}
    </div>

    <div v-else-if="!hasAudioSources" class="p-8 text-center">
      <AudioLines class="mx-auto text-base-content/35" :size="30" aria-hidden="true" />
      <p class="mt-3 text-sm font-medium">{{ t("No audio sources imported") }}</p>
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
              <span>{{ t(audioSource.sources.length === 1 ? "{count} catalog source" : "{count} catalog sources", { count: audioSource.sources.length }) }}</span>
              <span class="truncate" :title="audioSource.path">{{ audioSource.path }}</span>
            </div>

            <ChkszAudioSourceSettings
              v-if="audioSource.id === CHKSZ_AUDIO_SOURCE_ID"
            />

            <div v-if="audioSource.declaredCapabilities.length" class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">{{ t("Permissions") }}</h4>
                <span
                  v-if="audioSource.permissionsReviewed"
                  class="flex items-center gap-1 text-xs text-success"
                >
                  <ShieldCheck :size="14" aria-hidden="true" />
                  {{ t("Reviewed") }}
                </span>
                <span v-else class="text-xs text-warning">{{ t("Review required") }}</span>
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
                    :aria-label="t('Grant {capability}', { capability: capabilityLabel(capability) })"
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
                  {{ t("Confirm before enabling. This Audio Source can contact any network host through Fika.") }}
                </span>
                <button
                  class="btn btn-sm"
                  type="button"
                  :disabled="busySourceId === audioSource.id"
                  @click="reviewCapabilities(audioSource)"
                >
              <ShieldCheck :size="16" aria-hidden="true" />
                  {{ t("Confirm review") }}
                </button>
              </div>
            </div>

            <div class="space-y-2">
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <h4 class="text-sm font-semibold">{{ t("Catalog") }}</h4>
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :disabled="availabilityCheck !== null || busySourceId === audioSource.id"
                  @click="checkAvailability(audioSource)"
                >
                  <RefreshCw
                    :class="{ 'animate-spin': isCheckingAvailability(audioSource.id) }"
                    :size="15"
                    aria-hidden="true"
                  />
                  {{ t("Check all") }}
                </button>
              </div>
              <ul class="divide-y divide-base-300 border border-base-300">
                <li
                  v-for="source in audioSource.sources"
                  :key="source.id"
                  class="flex min-w-0 flex-col gap-2 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
                >
                  <div class="min-w-0">
                    <div class="truncate text-sm">{{ source.name }} / {{ source.id }}</div>
                    <div class="truncate text-xs text-muted">
                      {{ source.qualities.length ? source.qualities.join(", ") : t("Default quality") }}
                    </div>
                    <div
                      v-if="availabilityFor(audioSource.id, source.id)?.message"
                      class="mt-1 break-words text-xs text-error"
                    >
                      {{ availabilityFor(audioSource.id, source.id)?.message }}
                    </div>
                  </div>
                  <div class="flex shrink-0 items-center gap-3">
                    <span
                      v-if="availabilityFor(audioSource.id, source.id)"
                      class="flex items-center gap-1 text-xs"
                      :class="availabilityClass(availabilityFor(audioSource.id, source.id))"
                    >
                      <CircleCheck
                        v-if="availabilityFor(audioSource.id, source.id)?.available"
                        :size="14"
                        aria-hidden="true"
                      />
                      <CircleX v-else :size="14" aria-hidden="true" />
                      {{ t(availabilityFor(audioSource.id, source.id)?.available ? "Available" : "Unavailable") }}
                    </span>
                    <span v-if="availabilityFor(audioSource.id, source.id)" class="text-xs text-muted">
                      {{ availabilityFor(audioSource.id, source.id)?.latencyMs }} ms
                    </span>
                    <button
                      class="btn btn-ghost btn-sm"
                      type="button"
                      :disabled="availabilityCheck !== null || busySourceId === audioSource.id"
                      :aria-label="t('Check {name}', { name: source.name })"
                      @click="checkAvailability(audioSource, source.id)"
                    >
                      <RefreshCw
                        :class="{ 'animate-spin': isCheckingAvailability(audioSource.id, source.id) }"
                        :size="15"
                        aria-hidden="true"
                      />
                      {{ t("Check") }}
                    </button>
                  </div>
                </li>
              </ul>
            </div>

            <div class="space-y-2">
              <div class="flex items-center justify-between gap-3">
                <h4 class="text-sm font-semibold">{{ t("Diagnostics") }}</h4>
                <button
                  v-if="audioSource.diagnostics.length"
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :disabled="busySourceId === audioSource.id"
                  @click="clearDiagnostics(audioSource)"
                >
                  {{ t("Clear") }}
                </button>
              </div>
              <p v-if="!audioSource.diagnostics.length" class="text-xs text-muted">
                {{ t("No diagnostics.") }}
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
            <span class="sr-only">{{ t("Enable {name}", { name: audioSource.name }) }}</span>
            <input
            class="toggle toggle-md"
              type="checkbox"
              :checked="audioSource.enabled"
              :disabled="busySourceId === audioSource.id || (!audioSource.enabled && !audioSource.canEnable)"
              :aria-label="t('Enable {name}', { name: audioSource.name })"
              @change="toggleEnabled(audioSource)"
            />
          </label>
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :aria-label="expandedSourceId === audioSource.id ? t('Collapse {name}', { name: audioSource.name }) : t('Inspect {name}', { name: audioSource.name })"
            :title="expandedSourceId === audioSource.id ? t('Collapse details') : t('Inspect details')"
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
            :aria-label="t('Remove {name}', { name: audioSource.name })"
            :title="t('Remove audio source')"
            @click="requestRemoveSource(audioSource)"
          >
            <Trash2 :size="16" aria-hidden="true" />
          </button>
        </div>
      </li>
    </ul>
  </section>
</template>
