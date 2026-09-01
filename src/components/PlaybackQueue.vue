<script setup lang="ts">
import { computed, ref } from "vue";
import { GripVertical, ListMusic, LoaderCircle, Trash2, X } from "@lucide/vue";
import { formatNumber, t } from "../i18n";
import {
  playbackQueueItemSubtitle,
  playbackQueueItemTitle,
  type PlaybackQueueItem,
} from "../lib/playback-queue";

const props = defineProps<{
  open: boolean;
  items: PlaybackQueueItem[];
  total?: number;
  loading?: boolean;
  canLoadMore?: boolean;
}>();

const emit = defineEmits<{
  close: [];
  clear: [];
  play: [index: number];
  remove: [index: number];
  move: [from: number, to: number];
  loadMore: [];
}>();

const draggedIndex = ref<number | null>(null);
const queueCount = computed(() => props.total ?? props.items.length);
const queueCountLabel = computed(() => t(
  queueCount.value === 1 ? "{count} track in queue" : "{count} tracks in queue",
  { count: formatNumber(queueCount.value) },
));

function startDrag(event: DragEvent, index: number) {
  if (props.items[index]?.context) return;
  draggedIndex.value = index;
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
  }
}

function dropItem(event: DragEvent, index: number) {
  event.preventDefault();
  if (props.items[index]?.context) return;
  const from = draggedIndex.value;
  draggedIndex.value = null;
  if (from === null || from === index || from < 0 || from >= props.items.length) return;
  emit("move", from, index);
}

function finishDrag() {
  draggedIndex.value = null;
}
</script>

<template>
  <dialog
    v-if="open"
    open
    class="modal modal-end"
    aria-labelledby="playback-queue-title"
    @cancel.prevent="emit('close')"
  >
    <div class="modal-box flex max-h-[min(42rem,calc(100vh-2rem))] w-full max-w-lg flex-col gap-4 rounded p-0">
      <div class="flex items-start gap-3 border-b border-base-300 px-5 py-4">
        <div class="flex min-w-0 flex-1 items-start gap-3">
          <ListMusic class="mt-0.5 shrink-0" :size="20" aria-hidden="true" />
          <div class="min-w-0">
            <h2 id="playback-queue-title" class="text-base font-semibold">{{ t("Playback queue") }}</h2>
            <p class="mt-0.5 text-xs text-muted">{{ queueCountLabel }}</p>
          </div>
        </div>
        <button
          class="btn btn-square btn-ghost btn-sm shrink-0"
          type="button"
          :aria-label="t('Close playback queue')"
          :title="t('Close')"
          @click="emit('close')"
        >
          <X :size="17" aria-hidden="true" />
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <div v-if="!items.length" class="flex min-h-40 flex-col items-center justify-center gap-2 px-4 text-center text-sm text-muted">
          <LoaderCircle v-if="loading" class="animate-spin" :size="28" aria-hidden="true" />
          <ListMusic v-else :size="28" aria-hidden="true" />
          <span>{{ t(loading ? "Loading queue" : "Queue is empty") }}</span>
        </div>

        <ul v-else class="list divide-y divide-base-300" :aria-label="t('Up next')">
          <li
            v-for="(item, index) in items"
            :key="item.id"
            class="list-row min-w-0 items-center gap-2 px-2 py-2"
            :class="{ 'cursor-grab active:cursor-grabbing': !item.context }"
            :draggable="!item.context"
            :data-playback-queue-index="index"
            @dragstart="startDrag($event, index)"
            @dragover.prevent
            @drop="dropItem($event, index)"
            @dragend="finishDrag"
          >
            <GripVertical v-if="!item.context" class="shrink-0 text-muted" :size="16" aria-hidden="true" />
            <span v-else class="w-4 shrink-0" aria-hidden="true"></span>
            <div class="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
              <img
                v-if="item.kind === 'online' && item.track.coverUrl"
                class="size-full object-cover"
                :src="item.track.coverUrl"
                alt=""
              />
              <ListMusic v-else :size="16" aria-hidden="true" />
            </div>
            <button
              class="list-col-grow min-w-0 text-left"
              type="button"
              :aria-label="t('Play {title}', { title: playbackQueueItemTitle(item) })"
              @click="emit('play', index)"
            >
              <span class="block truncate text-sm font-medium">{{ playbackQueueItemTitle(item) }}</span>
              <span class="block truncate text-xs text-muted">{{ playbackQueueItemSubtitle(item) }}</span>
            </button>
            <button
              v-if="!item.context"
              class="btn btn-square btn-ghost btn-sm shrink-0"
              type="button"
              :aria-label="t('Remove {title} from queue', { title: playbackQueueItemTitle(item) })"
              :title="t('Remove from queue')"
              @click="emit('remove', index)"
            >
              <Trash2 :size="16" aria-hidden="true" />
            </button>
          </li>
        </ul>
      </div>

      <div class="modal-action m-0 flex justify-end border-t border-base-300 px-5 py-3">
        <button
          v-if="canLoadMore"
          class="btn btn-ghost btn-sm"
          type="button"
          :disabled="loading"
          @click="emit('loadMore')"
        >
          <LoaderCircle v-if="loading" class="animate-spin" :size="16" aria-hidden="true" />
          {{ t("Load more") }}
        </button>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!queueCount"
          @click="emit('clear')"
        >
          <Trash2 :size="16" aria-hidden="true" />
          {{ t("Clear queue") }}
        </button>
      </div>
    </div>
    <button class="modal-backdrop" type="button" :aria-label="t('Close playback queue')" @click="emit('close')"></button>
  </dialog>
</template>
