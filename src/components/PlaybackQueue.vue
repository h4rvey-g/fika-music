<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { GripVertical, ListMusic, LoaderCircle, Trash2, Volume2, X } from "@lucide/vue";
import { formatNumber, t } from "../i18n";
import {
  playbackQueueItemSubtitle,
  playbackQueueItemTitle,
  type PlaybackQueueItem,
} from "../lib/playback-queue";
import PlaybackQueueArtwork from "./PlaybackQueueArtwork.vue";

const props = defineProps<{
  open: boolean;
  current: PlaybackQueueItem | null;
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

const viewport = ref<HTMLElement | null>(null);
const queueList = ref<HTMLElement | null>(null);
const selectedItemId = ref<string | null>(null);
const draggedIndex = ref<number | null>(null);
const dropIndex = ref<number | null>(null);
let drag: {
  pointerId: number;
  index: number;
  row: HTMLElement;
  startX: number;
  startY: number;
  x: number;
  y: number;
} | null = null;
let scrollFrame: number | null = null;
let clickResetTimer: ReturnType<typeof setTimeout> | null = null;
let suppressClick = false;
const queueCount = computed(() => props.total ?? props.items.length);
const queueCountLabel = computed(() => t(
  queueCount.value === 1 ? "{count} track in queue" : "{count} tracks in queue",
  { count: formatNumber(queueCount.value) },
));

function selectItem(index: number) {
  if (!props.loading) selectedItemId.value = props.items[index]?.id ?? null;
}

function playItem(index: number) {
  if (props.loading || suppressClick) return;
  selectItem(index);
  emit("play", index);
}

function ignoreDragClick(event: MouseEvent) {
  if (!suppressClick) return;
  event.preventDefault();
  event.stopPropagation();
}

function startDrag(event: PointerEvent, index: number) {
  suppressClick = false;
  if (props.loading || event.isPrimary === false || event.button !== 0) return;
  const target = event.target as Element;
  if (target.closest("[data-queue-remove]")) return;
  // Touch scrolling stays available on the row; its grip starts touch reordering.
  if (event.pointerType === "touch" && !target.closest("[data-queue-drag-handle]")) return;
  const row = event.currentTarget as HTMLElement;
  drag = {
    pointerId: event.pointerId, index, row,
    startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY,
  };
  row.setPointerCapture?.(event.pointerId);
}

function updateDropIndex() {
  if (!drag) return;
  const row = document.elementFromPoint(drag.x, drag.y)?.closest<HTMLElement>("[data-playback-queue-index]");
  dropIndex.value = row && queueList.value?.contains(row)
    ? Number(row.dataset.playbackQueueIndex)
    : null;
}

function scrollWhileDragging() {
  if (!drag || !viewport.value) return;
  const bounds = viewport.value.getBoundingClientRect();
  if (drag.x >= bounds.left && drag.x <= bounds.right && drag.y >= bounds.top && drag.y <= bounds.bottom) {
    const speed = drag.y < bounds.top + 32 ? -10 : drag.y > bounds.bottom - 32 ? 10 : 0;
    if (speed) {
      viewport.value.scrollTop += speed;
      updateDropIndex();
    }
  }
  scrollFrame = requestAnimationFrame(scrollWhileDragging);
}

function moveDrag(event: PointerEvent) {
  if (!drag || event.pointerId !== drag.pointerId) return;
  drag.x = event.clientX;
  drag.y = event.clientY;
  if (draggedIndex.value === null) {
    if (Math.hypot(drag.x - drag.startX, drag.y - drag.startY) < 5) return;
    draggedIndex.value = drag.index;
    selectItem(drag.index);
    scrollFrame = requestAnimationFrame(scrollWhileDragging);
  }
  event.preventDefault();
  updateDropIndex();
}

function cancelDrag() {
  const previous = drag;
  if (draggedIndex.value !== null) {
    suppressClick = true;
    if (clickResetTimer !== null) clearTimeout(clickResetTimer);
    // Ignore the click synthesized by pointerup, then allow keyboard/AX activation.
    clickResetTimer = setTimeout(() => {
      suppressClick = false;
      clickResetTimer = null;
    }, 0);
  }
  drag = null;
  draggedIndex.value = null;
  dropIndex.value = null;
  if (scrollFrame !== null) cancelAnimationFrame(scrollFrame);
  scrollFrame = null;
  if (previous?.row.hasPointerCapture?.(previous.pointerId)) {
    previous.row.releasePointerCapture(previous.pointerId);
  }
}

function finishDrag(event: PointerEvent) {
  if (!drag || event.pointerId !== drag.pointerId) return;
  drag.x = event.clientX;
  drag.y = event.clientY;
  const from = draggedIndex.value;
  if (from !== null) updateDropIndex();
  const to = dropIndex.value;
  cancelDrag();
  if (!props.loading && from !== null && to !== null && from !== to) emit("move", from, to);
}

watch(() => props.items.map((item) => item.id), (ids, previous) => {
  if (ids.length !== previous.length || ids.some((id, index) => id !== previous[index])) cancelDrag();
  if (selectedItemId.value && !ids.includes(selectedItemId.value)) selectedItemId.value = null;
});
watch([() => props.open, () => props.loading], () => {
  cancelDrag();
  if (!props.open) selectedItemId.value = null;
});
onBeforeUnmount(() => {
  cancelDrag();
  if (clickResetTimer !== null) clearTimeout(clickResetTimer);
});
</script>

<template>
  <dialog
    v-if="open"
    open
    class="modal modal-end"
    aria-labelledby="playback-queue-title"
    @cancel.prevent="emit('close')"
  >
    <div class="modal-box playback-queue-box flex max-h-[min(42rem,calc(100vh-2rem))] w-full max-w-lg! flex-col gap-4 rounded p-0">
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

      <div ref="viewport" class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <div v-if="current" class="mb-2 border-b border-base-300 px-2 py-3">
          <p class="mb-2 text-xs font-semibold uppercase text-muted">{{ t("Now playing") }}</p>
          <div class="flex min-w-0 items-center gap-2">
            <Volume2 class="shrink-0 text-primary" :size="16" aria-hidden="true" />
            <PlaybackQueueArtwork :key="current.id" :item="current" />
            <div class="min-w-0 flex-1">
              <span class="block truncate text-sm font-medium">{{ playbackQueueItemTitle(current) }}</span>
              <span class="block truncate text-xs text-muted">{{ playbackQueueItemSubtitle(current) }}</span>
            </div>
          </div>
        </div>

        <div v-if="!items.length" class="flex min-h-40 flex-col items-center justify-center gap-2 px-4 text-center text-sm text-muted">
          <LoaderCircle v-if="loading" class="animate-spin" :size="28" aria-hidden="true" />
          <ListMusic v-else :size="28" aria-hidden="true" />
          <span>{{ t(loading ? "Loading queue" : "Queue is empty") }}</span>
        </div>

        <ul ref="queueList" v-else class="list divide-y divide-base-300" :aria-label="t('Up next')">
          <li
            v-for="(item, index) in items"
            :key="item.id"
            class="list-row relative min-w-0 select-none items-center gap-2 px-2 py-2"
            :class="{
              'cursor-grab active:cursor-grabbing': !loading,
              'bg-base-300': selectedItemId === item.id,
              'opacity-50': draggedIndex === index,
            }"
            :data-playback-queue-index="index"
            @pointerdown="startDrag($event, index)"
            @pointermove="moveDrag"
            @pointerup="finishDrag"
            @pointercancel="cancelDrag"
            @lostpointercapture="cancelDrag"
            @dragstart.prevent
            @click.capture="ignoreDragClick"
            @click="selectItem(index)"
            @dblclick="playItem(index)"
          >
            <span data-queue-drag-handle class="touch-none" :title="t('Drag to reorder')">
              <GripVertical class="shrink-0 text-muted" :size="16" aria-hidden="true" />
            </span>
            <PlaybackQueueArtwork :item="item" />
            <button
              class="list-col-grow min-w-0 text-left"
              type="button"
              :disabled="loading"
              :aria-label="t('Select {title}', { title: playbackQueueItemTitle(item) })"
              :aria-pressed="selectedItemId === item.id"
              :title="t('Double-click to play')"
              @keydown.enter.stop.prevent="playItem(index)"
            >
              <span class="block truncate text-sm font-medium">{{ playbackQueueItemTitle(item) }}</span>
              <span class="block truncate text-xs text-muted">{{ playbackQueueItemSubtitle(item) }}</span>
            </button>
            <button
              class="btn btn-square btn-ghost btn-sm shrink-0"
              data-queue-remove
              type="button"
              :disabled="loading"
              :aria-label="t('Remove {title} from queue', { title: playbackQueueItemTitle(item) })"
              :title="t('Remove from queue')"
              @click.stop="emit('remove', index)"
              @dblclick.stop
            >
              <X :size="16" aria-hidden="true" />
            </button>
            <span
              v-if="dropIndex === index && draggedIndex !== null && draggedIndex !== index"
              class="pointer-events-none absolute inset-x-0 h-0.5 bg-primary"
              :class="draggedIndex < index ? 'bottom-0' : 'top-0'"
              aria-hidden="true"
            ></span>
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
          :disabled="!queueCount || loading"
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
