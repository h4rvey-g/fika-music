<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  ArrowDown,
  ArrowUp,
  Download,
  FolderOpen,
  History,
  Radio,
  Save,
  Trash2,
} from "@lucide/vue";
import type { AudioSourceRecord, OnlineChannel } from "../generated/bindings";
import { normalizeError } from "../lib/errors";
import { t } from "../i18n";
import {
  clearOnlineSearchHistory,
  getOnlineMusicSettings,
  listOnlineMusicChannels,
  selectOnlineDownloadDirectory,
  updateOnlineMusicSettings,
  type OnlineMusicSettings,
} from "../lib/online-music-api";

const props = defineProps<{ audioSources: AudioSourceRecord[] }>();
const emit = defineEmits<{ settingsChanged: [settings: OnlineMusicSettings] }>();

const settings = ref<OnlineMusicSettings | null>(null);
const channels = ref<OnlineChannel[]>([]);
const templateDraft = ref("");
const templateError = ref<string | null>(null);
const error = ref<string | null>(null);
const saving = ref<string | null>(null);
const draggedChannelId = ref<string | null>(null);
const draggedAudioSourceId = ref<string | null>(null);
const qualityOptions: ReadonlyArray<{
  value: OnlineMusicSettings["playbackQuality"];
  label: string;
}> = [
  { value: "128k", label: "128 kbps" },
  { value: "320k", label: "320 kbps" },
  { value: "flac", label: "FLAC" },
  { value: "flac24bit", label: "FLAC 24-bit" },
];

const orderedChannels = computed(() => {
  if (!settings.value) return channels.value;
  return orderedBy(channels.value, settings.value.channelPriority, (channel) => channel.id);
});

const eligibleAudioSources = computed(() =>
  props.audioSources.filter(
    (record) =>
      record.enabled &&
      record.state === "enabled" &&
      record.sources.some((source) => source.actions.includes("musicUrl")),
  ),
);

const orderedAudioSources = computed(() => {
  if (!settings.value) return eligibleAudioSources.value;
  return orderedBy(
    eligibleAudioSources.value,
    settings.value.audioSourcePriority,
    (source) => source.id,
  );
});

const templatePreview = computed(() => {
  const sample = {
    artist: t("Artist"),
    title: t("Song title"),
    album: t("Album"),
    trackNumber: "02",
    channel: t("Channel"),
  };
  try {
    return previewTemplate(templateDraft.value, sample);
  } catch (reason) {
    return reason instanceof Error ? reason.message : t("Invalid filename template");
  }
});

onMounted(async () => {
  try {
    const [loadedSettings, loadedChannels] = await Promise.all([
      getOnlineMusicSettings(),
      listOnlineMusicChannels(true),
    ]);
    settings.value = loadedSettings;
    channels.value = Array.isArray(loadedChannels) ? loadedChannels : [];
    templateDraft.value = settings.value.filenameTemplate;
  } catch (reason) {
    error.value = normalizeError(reason);
  }
});

async function persist(patch: Partial<OnlineMusicSettings>, key: string) {
  if (!settings.value || saving.value) return;
  const previous = settings.value;
  const next = { ...previous, ...patch };
  settings.value = next;
  saving.value = key;
  error.value = null;
  try {
    settings.value = await updateOnlineMusicSettings(next);
    emit("settingsChanged", settings.value);
  } catch (reason) {
    settings.value = previous;
    error.value = normalizeError(reason);
  } finally {
    saving.value = null;
  }
}

function toggleChannel(channel: OnlineChannel, included: boolean) {
  if (!settings.value) return;
  const excluded = new Set(settings.value.excludedChannels);
  if (included) excluded.delete(channel.id);
  else excluded.add(channel.id);
  void persist({ excludedChannels: [...excluded] }, `channel-${channel.id}`);
}

function moveChannel(index: number, offset: number) {
  if (!settings.value) return;
  const ids = orderedChannels.value.map((channel) => channel.id);
  move(ids, index, offset);
  void persist({ channelPriority: ids }, "channel-priority");
}

function moveAudioSource(index: number, offset: number) {
  if (!settings.value) return;
  const ids = orderedAudioSources.value.map((source) => source.id);
  move(ids, index, offset);
  void persist({ audioSourcePriority: ids }, "audio-priority");
}

function dropChannel(targetId: string) {
  if (!settings.value || !draggedChannelId.value || draggedChannelId.value === targetId) return;
  const ids = orderedChannels.value.map((channel) => channel.id);
  moveBefore(ids, draggedChannelId.value, targetId);
  draggedChannelId.value = null;
  void persist({ channelPriority: ids }, "channel-priority");
}

function dropAudioSource(targetId: string) {
  if (!settings.value || !draggedAudioSourceId.value || draggedAudioSourceId.value === targetId) return;
  const ids = orderedAudioSources.value.map((source) => source.id);
  moveBefore(ids, draggedAudioSourceId.value, targetId);
  draggedAudioSourceId.value = null;
  void persist({ audioSourcePriority: ids }, "audio-priority");
}

async function chooseDirectory() {
  if (!settings.value) return;
  const directory = await selectOnlineDownloadDirectory();
  if (directory) await persist({ downloadDirectory: directory }, "directory");
}

async function applyTemplate() {
  templateError.value = null;
  try {
    previewTemplate(templateDraft.value, {
      artist: t("Artist"),
      title: t("Song title"),
      album: t("Album"),
      trackNumber: "02",
      channel: t("Channel"),
    });
  } catch (reason) {
    templateError.value = normalizeError(reason);
    return;
  }
  await persist({ filenameTemplate: templateDraft.value }, "template");
}

async function toggleHistory(enabled: boolean) {
  await persist({ searchHistoryEnabled: enabled }, "history");
}

async function clearHistory() {
  saving.value = "clear-history";
  try {
    await clearOnlineSearchHistory();
  } catch (reason) {
    error.value = normalizeError(reason);
  } finally {
    saving.value = null;
  }
}

function orderedBy<T>(items: T[], priority: string[], id: (item: T) => string) {
  return [...items].sort((left, right) => {
    const leftIndex = priority.indexOf(id(left));
    const rightIndex = priority.indexOf(id(right));
    const leftRank = leftIndex < 0 ? priority.length : leftIndex;
    const rightRank = rightIndex < 0 ? priority.length : rightIndex;
    return leftRank - rightRank || id(left).localeCompare(id(right));
  });
}

function move(items: string[], index: number, offset: number) {
  const target = index + offset;
  if (target < 0 || target >= items.length) return;
  [items[index], items[target]] = [items[target], items[index]];
}

function moveBefore(items: string[], dragged: string, target: string) {
  const from = items.indexOf(dragged);
  const to = items.indexOf(target);
  if (from < 0 || to < 0) return;
  items.splice(from, 1);
  items.splice(items.indexOf(target), 0, dragged);
}

function previewTemplate(template: string, values: Record<string, string>) {
  if (!template.trim() || template.length > 512 || !template.includes("{title}")) {
    throw new Error(t("Template must include {title} and contain at most 512 characters."));
  }
  let output = template.replace(/\[((?:\\.|[^\]])*)\]/g, (_, group: string) => {
    const fields = [...group.matchAll(/\{([^}]+)\}/g)].map((match) => match[1]);
    return fields.every((field) => values[field]?.trim()) ? group : "";
  });
  output = output.replace(/\{([^}]+)\}/g, (_, field: string) => {
    if (!(field in values)) {
      throw new Error(t("Unsupported field {field}.", { field: `{${field}}` }));
    }
    return values[field];
  });
  output = output.replace(/\\([\[\]\\])/g, "$1");
  output = output.replace(/[\\/:*?"<>|\u0000-\u001f]/g, " ").replace(/\s+/g, " ").trim();
  if (!output) throw new Error(t("Template produces an empty filename."));
  return output;
}

</script>

<template>
  <div v-if="!settings" class="space-y-3">
    <div v-for="index in 3" :key="index" class="skeleton h-28 w-full"></div>
  </div>
  <div v-else class="flex flex-col gap-4">
    <div v-if="error" role="alert" class="alert alert-error py-2 text-sm">{{ error }}</div>

    <section class="overflow-hidden rounded border border-base-300 bg-base-100">
      <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
        <Radio :size="18" aria-hidden="true" />
        <h2 class="text-base font-semibold">{{ t("Online Music") }}</h2>
      </div>
      <div class="divide-y divide-base-300">
        <div class="px-4 py-4">
          <div class="mb-3 text-sm font-medium">{{ t("Search channels and priority") }}</div>
          <ul class="divide-y divide-base-300">
            <li
              v-for="(channel, index) in orderedChannels"
              :key="channel.id"
              class="flex min-w-0 items-center gap-2 py-2"
              draggable="true"
              @dragstart="draggedChannelId = channel.id"
              @dragover.prevent
              @drop="dropChannel(channel.id)"
            >
              <input
                class="checkbox checkbox-md"
                type="checkbox"
                :checked="!settings.excludedChannels.includes(channel.id)"
                :aria-label="t('Include {name}', { name: channel.sourceName })"
                @change="toggleChannel(channel, ($event.currentTarget as HTMLInputElement).checked)"
              />
              <div class="min-w-0 flex-1">
                <div class="truncate text-sm">{{ channel.sourceName }}</div>
                <div class="truncate text-xs text-muted">{{ channel.pluginName }} · {{ channel.sourceId }}</div>
              </div>
              <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="index === 0 || Boolean(saving)" :aria-label="t('Move channel up')" :title="t('Move up')" @click="moveChannel(index, -1)">
                <ArrowUp :size="16" aria-hidden="true" />
              </button>
              <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="index === orderedChannels.length - 1 || Boolean(saving)" :aria-label="t('Move channel down')" :title="t('Move down')" @click="moveChannel(index, 1)">
                <ArrowDown :size="16" aria-hidden="true" />
              </button>
            </li>
          </ul>
        </div>

        <div class="px-4 py-4">
          <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div class="min-w-0">
              <div class="text-sm font-medium">{{ t("Audio Source selection") }}</div>
            </div>
            <div class="join shrink-0" role="radiogroup" :aria-label="t('Audio Source selection mode')">
              <input
                class="btn btn-sm join-item"
                type="radio"
                name="audio-source-selection-mode"
                value="automatic"
                :aria-label="t('Automatic')"
                :checked="settings.audioSourceSelectionMode === 'automatic'"
                :disabled="Boolean(saving)"
                @change="persist({ audioSourceSelectionMode: 'automatic' }, 'audio-source-mode')"
              />
              <input
                class="btn btn-sm join-item"
                type="radio"
                name="audio-source-selection-mode"
                value="manual"
                :aria-label="t('Manual')"
                :checked="settings.audioSourceSelectionMode === 'manual'"
                :disabled="Boolean(saving)"
                @change="persist({ audioSourceSelectionMode: 'manual' }, 'audio-source-mode')"
              />
            </div>
          </div>
        </div>

        <div v-if="settings.audioSourceSelectionMode === 'manual'" class="px-4 py-4">
          <div class="mb-3 text-sm font-medium">{{ t("Manual fallback priority") }}</div>
          <ul class="divide-y divide-base-300" data-testid="audio-source-priority">
            <li
              v-for="(source, index) in orderedAudioSources"
              :key="source.id"
              class="flex min-w-0 items-center gap-2 py-2"
              draggable="true"
              @dragstart="draggedAudioSourceId = source.id"
              @dragover.prevent
              @drop="dropAudioSource(source.id)"
            >
              <span class="w-6 text-center text-xs tabular-nums text-muted">{{ index + 1 }}</span>
              <span class="min-w-0 flex-1 truncate text-sm">{{ source.name }}</span>
              <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="index === 0 || Boolean(saving)" :aria-label="t('Move Audio Source up')" :title="t('Move up')" @click="moveAudioSource(index, -1)">
                <ArrowUp :size="16" aria-hidden="true" />
              </button>
              <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="index === orderedAudioSources.length - 1 || Boolean(saving)" :aria-label="t('Move Audio Source down')" :title="t('Move down')" @click="moveAudioSource(index, 1)">
                <ArrowDown :size="16" aria-hidden="true" />
              </button>
            </li>
          </ul>
        </div>

        <div class="grid gap-4 px-4 py-4 sm:grid-cols-3">
          <label class="form-control">
            <span class="label-text text-sm">{{ t("Playback quality") }}</span>
            <select
              data-testid="online-playback-quality"
              class="select select-sm mt-1"
              :value="settings.playbackQuality"
              @change="persist({ playbackQuality: ($event.currentTarget as HTMLSelectElement).value as OnlineMusicSettings['playbackQuality'] }, 'playback-quality')"
            >
              <option v-for="quality in qualityOptions" :key="quality.value" :value="quality.value">{{ quality.label }}</option>
            </select>
          </label>
          <label class="form-control">
            <span class="label-text text-sm">{{ t("Per-source budget") }}</span>
            <input class="input input-sm mt-1" type="number" min="3" max="30" :value="settings.layerTimeoutSeconds" @change="persist({ layerTimeoutSeconds: Number(($event.currentTarget as HTMLInputElement).value) }, 'layer-timeout')" />
          </label>
          <label class="form-control">
            <span class="label-text text-sm">{{ t("Playback timeout") }}</span>
            <input class="input input-sm mt-1" type="number" min="5" max="60" :value="settings.playbackTimeoutSeconds" @change="persist({ playbackTimeoutSeconds: Number(($event.currentTarget as HTMLInputElement).value) }, 'playback-timeout')" />
          </label>
        </div>

        <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
          <label class="flex min-w-0 items-center gap-3">
            <History :size="17" aria-hidden="true" />
            <span><span class="block text-sm font-medium">{{ t("Recent search history") }}</span><span class="block text-xs text-muted">{{ t("Stores up to 10 query strings locally") }}</span></span>
          </label>
          <div class="flex items-center gap-2">
          <button class="btn btn-sm" type="button" :disabled="saving === 'clear-history'" @click="clearHistory"><Trash2 :size="16" aria-hidden="true" />{{ t("Clear") }}</button>
          <input class="toggle toggle-md" type="checkbox" :checked="settings.searchHistoryEnabled" :aria-label="t('Store recent searches')" @change="toggleHistory(($event.currentTarget as HTMLInputElement).checked)" />
          </div>
        </div>
      </div>
    </section>

    <section class="overflow-hidden rounded border border-base-300 bg-base-100">
      <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
        <Download :size="18" aria-hidden="true" />
        <h2 class="text-base font-semibold">{{ t("Downloads") }}</h2>
      </div>
      <div class="divide-y divide-base-300">
        <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
          <div class="min-w-0"><div class="text-sm font-medium">{{ t("Download directory") }}</div><div class="truncate text-xs text-muted" :title="settings.downloadDirectory || undefined">{{ settings.downloadDirectory || t('Not configured') }}</div></div>
        <button class="btn btn-sm shrink-0" type="button" :disabled="saving === 'directory'" @click="chooseDirectory"><FolderOpen :size="16" aria-hidden="true" />{{ t("Choose") }}</button>
        </div>
        <div class="px-4 py-4">
          <label for="filename-template" class="text-sm font-medium">{{ t("Filename template") }}</label>
          <div class="mt-2 flex gap-2">
            <input id="filename-template" v-model="templateDraft" class="input input-sm min-w-0 flex-1 font-mono" />
          <button class="btn btn-primary btn-sm" type="button" :disabled="saving === 'template' || templateDraft === settings.filenameTemplate" @click="applyTemplate"><Save :size="16" aria-hidden="true" />{{ t("Apply") }}</button>
          </div>
          <div class="mt-1 truncate text-xs" :class="templateError ? 'text-error' : 'text-muted'">{{ templateError || templatePreview }}</div>
        </div>
        <div class="grid gap-4 px-4 py-4 sm:grid-cols-3">
          <label class="form-control">
            <span class="label-text text-sm">{{ t("Download quality") }}</span>
            <select
              data-testid="online-download-quality"
              class="select select-sm mt-1"
              :value="settings.downloadQuality"
              @change="persist({ downloadQuality: ($event.currentTarget as HTMLSelectElement).value as OnlineMusicSettings['downloadQuality'] }, 'download-quality')"
            >
              <option v-for="quality in qualityOptions" :key="quality.value" :value="quality.value">{{ quality.label }}</option>
            </select>
          </label>
          <label class="form-control"><span class="label-text text-sm">{{ t("Concurrent songs") }}</span><input class="input input-sm mt-1" type="number" min="1" max="4" :value="settings.downloadConcurrency" @change="persist({ downloadConcurrency: Number(($event.currentTarget as HTMLInputElement).value) }, 'download-concurrency')" /></label>
        <label class="flex items-center justify-between gap-3 self-end py-2"><span class="text-sm">{{ t("Batch completion notifications") }}</span><input class="toggle toggle-md" type="checkbox" :checked="settings.batchNotifications" @change="persist({ batchNotifications: ($event.currentTarget as HTMLInputElement).checked }, 'notifications')" /></label>
        </div>
      </div>
    </section>
  </div>
</template>
