<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Disc3, RefreshCw, Settings } from "@lucide/vue";
import type { ResolvedLyrics } from "../generated/bindings";
import {
  DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
  nowPlayingLyricsColor,
  nowPlayingLyricsFontFamily,
  type NowPlayingLyricsPreferences,
} from "../lib/now-playing-lyrics";
import { t } from "../i18n";

const props = withDefaults(defineProps<{
  title: string;
  subtitle: string;
  coverUrl: string | null;
  lyrics: ResolvedLyrics | null;
  lyricsLoading: boolean;
  lyricsError: string | null;
  playbackPosition: number;
  canRetry: boolean;
  fillHeight?: boolean;
  lyricsPreferences?: NowPlayingLyricsPreferences;
}>(), {
  fillHeight: false,
  lyricsPreferences: () => ({ ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES }),
});

const emit = defineEmits<{
  retryLyrics: [];
  seekPlayback: [position: number];
  openLyricsSettings: [];
}>();

const lyricsViewport = ref<HTMLElement | null>(null);
const isDraggingLyrics = ref(false);
const draggedLyricIndex = ref(-1);
const lyricsContextMenu = ref<{ x: number; y: number } | null>(null);
const lyricsRetryMenuAction = ref<HTMLButtonElement | null>(null);
const lyricsAppearanceMenuAction = ref<HTMLButtonElement | null>(null);

interface LyricDragState {
  pointerId: number;
  startY: number;
  startScrollTop: number;
}

let lyricDragState: LyricDragState | null = null;

const canDragLyrics = computed(() =>
  Boolean(
    props.lyrics?.isSynced
      && props.lyrics.lines.some((line) => line.startMs !== null),
  ),
);

const activeLyricIndex = computed(() => {
  if (!props.lyrics?.isSynced) {
    return -1;
  }

  const positionMs = props.playbackPosition * 1_000;
  for (let index = props.lyrics.lines.length - 1; index >= 0; index -= 1) {
    const startMs = props.lyrics.lines[index].startMs;
    if (startMs !== null && startMs <= positionMs) {
      return index;
    }
  }
  return -1;
});

const displayedLyricIndex = computed(() =>
  isDraggingLyrics.value ? draggedLyricIndex.value : activeLyricIndex.value,
);

const draggedLyricTimeLabel = computed(() => {
  const startMs = props.lyrics?.lines[draggedLyricIndex.value]?.startMs;
  if (startMs === null || startMs === undefined) {
    return null;
  }

  const totalSeconds = Math.floor(Math.max(0, startMs) / 1_000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
});

const lyricsTextStyle = computed(() => ({
  fontFamily: nowPlayingLyricsFontFamily(props.lyricsPreferences.font),
  textAlign: props.lyricsPreferences.alignment,
}));

function closestLyricIndexToViewportCenter(viewport: HTMLElement) {
  if (!props.lyrics) {
    return -1;
  }

  const viewportCenter = viewport.scrollTop + viewport.clientHeight / 2;
  let closestIndex = -1;
  let closestDistance = Number.POSITIVE_INFINITY;

  props.lyrics.lines.forEach((line, index) => {
    if (line.startMs === null) {
      return;
    }
    const element = viewport.querySelector<HTMLElement>(`[data-lyric-index="${index}"]`);
    if (!element) {
      return;
    }
    const lineCenter = element.offsetTop + element.offsetHeight / 2;
    const distance = Math.abs(lineCenter - viewportCenter);
    if (distance < closestDistance) {
      closestDistance = distance;
      closestIndex = index;
    }
  });

  return closestIndex;
}

function releaseLyricPointer(viewport: HTMLElement, pointerId: number) {
  if (viewport.hasPointerCapture?.(pointerId)) {
    viewport.releasePointerCapture(pointerId);
  }
}

function resetLyricDrag() {
  const dragState = lyricDragState;
  const viewport = lyricsViewport.value;
  lyricDragState = null;
  isDraggingLyrics.value = false;
  draggedLyricIndex.value = -1;
  if (dragState && viewport) {
    releaseLyricPointer(viewport, dragState.pointerId);
  }
}

function lyricLineStyle(index: number) {
  const active = index === displayedLyricIndex.value;
  const preferences = props.lyricsPreferences;
  return {
    color: nowPlayingLyricsColor(
      active ? preferences.activeColor : preferences.inactiveColor,
    ),
    fontSize: `${preferences.fontSize}px`,
    fontWeight: active ? preferences.activeFontWeight : 400,
    lineHeight: `${Math.max(24, Math.round(preferences.fontSize * 1.4))}px`,
    opacity: active ? 1 : preferences.inactiveOpacity,
    paddingBlock: `${preferences.lineGap / 2}px`,
  };
}

function closeLyricsContextMenu() {
  lyricsContextMenu.value = null;
}

function openLyricsContextMenu(event: MouseEvent) {
  resetLyricDrag();
  const width = 224;
  const height = props.canRetry ? 80 : 48;
  lyricsContextMenu.value = {
    x: Math.max(8, Math.min(event.clientX, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(event.clientY, window.innerHeight - height - 8)),
  };
  void nextTick(() => {
    const action = props.canRetry && !props.lyricsLoading
      ? lyricsRetryMenuAction.value
      : lyricsAppearanceMenuAction.value;
    action?.focus();
  });
}

function retryLyrics() {
  closeLyricsContextMenu();
  emit("retryLyrics");
}

function openLyricsAppearanceSettings() {
  closeLyricsContextMenu();
  emit("openLyricsSettings");
}

function startLyricDrag(event: PointerEvent) {
  if (!canDragLyrics.value || event.isPrimary === false || event.button !== 0) {
    return;
  }

  const viewport = event.currentTarget as HTMLElement;
  lyricDragState = {
    pointerId: event.pointerId,
    startY: event.clientY,
    startScrollTop: viewport.scrollTop,
  };
  viewport.setPointerCapture?.(event.pointerId);
}

function moveLyricDrag(event: PointerEvent) {
  const dragState = lyricDragState;
  if (!dragState || event.pointerId !== dragState.pointerId) {
    return;
  }

  const deltaY = event.clientY - dragState.startY;
  if (!isDraggingLyrics.value && Math.abs(deltaY) < 4) {
    return;
  }

  event.preventDefault();
  const viewport = event.currentTarget as HTMLElement;
  const maxScrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
  viewport.scrollTop = Math.min(
    maxScrollTop,
    Math.max(0, dragState.startScrollTop - deltaY),
  );
  isDraggingLyrics.value = true;
  draggedLyricIndex.value = closestLyricIndexToViewportCenter(viewport);
}

function finishLyricDrag(event: PointerEvent) {
  const dragState = lyricDragState;
  if (!dragState || event.pointerId !== dragState.pointerId) {
    return;
  }

  const targetIndex = isDraggingLyrics.value ? draggedLyricIndex.value : -1;
  resetLyricDrag();
  if (targetIndex < 0) {
    return;
  }

  const startMs = props.lyrics?.lines[targetIndex]?.startMs;
  if (startMs !== null && startMs !== undefined) {
    emit("seekPlayback", startMs / 1_000);
  }
}

watch(
  () => props.lyrics,
  async () => {
    closeLyricsContextMenu();
    resetLyricDrag();
    await nextTick();
    if (lyricsViewport.value) {
      lyricsViewport.value.scrollTop = 0;
    }
  },
);

watch(activeLyricIndex, async (index) => {
  if (index < 0 || lyricDragState) {
    return;
  }
  await nextTick();
  const viewport = lyricsViewport.value;
  const activeLine = viewport?.querySelector<HTMLElement>(
    `[data-lyric-index="${index}"]`,
  );
  if (!viewport || !activeLine) {
    return;
  }
  const top = Math.max(
    0,
    activeLine.offsetTop - (viewport.clientHeight - activeLine.offsetHeight) / 2,
  );
  if (typeof viewport.scrollTo === "function") {
    viewport.scrollTo({ top, behavior: "smooth" });
  } else {
    viewport.scrollTop = top;
  }
});
</script>

<template>
  <section
    class="flex min-h-0 flex-col overflow-hidden rounded border border-base-300 bg-base-100"
    :class="fillHeight ? 'h-full' : 'h-[36rem]'"
    :aria-label="t('Now playing details')"
  >
    <div class="grid shrink-0 place-items-center border-b border-base-300 bg-base-200 p-4">
      <img
        v-if="coverUrl"
        class="aspect-square w-44 max-w-full rounded object-cover shadow-sm"
        :src="coverUrl"
        :alt="t('{title} cover', { title })"
      />
      <div
        v-else
        class="grid aspect-square w-44 max-w-full place-items-center rounded border border-base-300 bg-base-100 text-base-content/35"
        aria-hidden="true"
      >
        <Disc3 :size="52" :stroke-width="1.25" />
      </div>
    </div>

    <div class="shrink-0 border-b border-base-300 px-4 py-3">
      <div class="truncate text-sm font-semibold" :title="title">{{ title }}</div>
      <div class="mt-0.5 truncate text-xs text-muted" :title="subtitle">
        {{ subtitle }}
      </div>
    </div>

    <div class="relative h-64 min-h-0 flex-1">
      <div
        ref="lyricsViewport"
        class="relative h-full overflow-y-auto px-4 text-center"
        :class="[
          canDragLyrics ? 'cursor-grab touch-none select-none' : 'scroll-smooth',
          isDraggingLyrics ? 'cursor-grabbing scroll-auto' : '',
        ]"
        data-testid="lyrics-viewport"
        :data-seeking="isDraggingLyrics || undefined"
        aria-live="polite"
        @contextmenu.prevent.stop="openLyricsContextMenu"
        @pointerdown="startLyricDrag"
        @pointermove="moveLyricDrag"
        @pointerup="finishLyricDrag"
        @pointercancel="resetLyricDrag"
      >
        <div
          v-if="lyricsLoading"
          class="flex h-full min-h-48 items-center justify-center gap-2 text-sm text-muted"
          role="status"
        >
          <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
          {{ t("Loading lyrics") }}
        </div>

        <div
          v-else-if="lyricsError"
          class="grid h-full min-h-48 place-items-center py-8 text-sm text-error"
          role="alert"
        >
          <p class="max-w-56">{{ lyricsError }}</p>
        </div>

        <div
          v-else-if="!lyrics?.lines.length"
          class="grid h-full min-h-48 place-items-center py-8 text-sm text-muted"
        >
          {{ t("No lyrics available") }}
        </div>

        <div
          v-else
          class="min-h-full py-20"
          :style="lyricsTextStyle"
          data-testid="lyric-lines"
        >
          <p
            v-for="(line, index) in lyrics.lines"
            :key="`${line.startMs ?? 'plain'}-${index}`"
            class="break-words whitespace-pre-line transition-colors duration-200"
            :style="lyricLineStyle(index)"
            :data-lyric-index="index"
            :data-active="index === displayedLyricIndex || undefined"
          >
            {{ line.text }}
          </p>
        </div>
      </div>

      <div
        v-if="isDraggingLyrics"
        class="pointer-events-none absolute inset-x-4 top-1/2 flex -translate-y-1/2 items-center gap-2"
        data-testid="lyric-seek-guide"
      >
        <span class="h-px min-w-0 flex-1 bg-base-content/20" aria-hidden="true" />
        <output
          v-if="draggedLyricTimeLabel"
          class="bg-base-100 px-1 text-xs tabular-nums text-muted"
          data-testid="lyric-seek-time"
        >
          {{ draggedLyricTimeLabel }}
        </output>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="lyricsContextMenu"
        class="fixed inset-0 z-40"
        data-lyrics-context-backdrop
        aria-hidden="true"
        @pointerdown="closeLyricsContextMenu"
        @contextmenu.prevent="closeLyricsContextMenu"
      ></div>
      <ul
        v-if="lyricsContextMenu"
        class="menu menu-sm fixed z-50 w-56 rounded border border-base-300 bg-base-100 p-2 text-base-content shadow-xl"
        :style="{
          left: `${lyricsContextMenu.x}px`,
          top: `${lyricsContextMenu.y}px`,
        }"
        data-lyrics-context-menu
        :aria-label="t('Lyrics actions')"
        @keydown.esc.stop.prevent="closeLyricsContextMenu"
      >
        <li v-if="canRetry">
          <button
            ref="lyricsRetryMenuAction"
            type="button"
            :disabled="lyricsLoading"
            @click="retryLyrics"
          >
            <RefreshCw
              :class="{ 'animate-spin': lyricsLoading }"
              :size="16"
              aria-hidden="true"
            />
            {{ t("Retry lyrics") }}
          </button>
        </li>
        <li>
          <button
            ref="lyricsAppearanceMenuAction"
            type="button"
            @click="openLyricsAppearanceSettings"
          >
            <Settings :size="16" aria-hidden="true" />
            {{ t("Lyrics appearance") }}
          </button>
        </li>
      </ul>
    </Teleport>
  </section>
</template>
