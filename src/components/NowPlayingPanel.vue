<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Disc3, FileText, RefreshCw } from "@lucide/vue";
import type { ResolvedLyrics } from "../generated/bindings";

const props = defineProps<{
  title: string;
  subtitle: string;
  coverUrl: string | null;
  lyrics: ResolvedLyrics | null;
  lyricsLoading: boolean;
  lyricsError: string | null;
  playbackPosition: number;
  canRetry: boolean;
  fillHeight?: boolean;
}>();

const emit = defineEmits<{
  retryLyrics: [];
}>();

const lyricsViewport = ref<HTMLElement | null>(null);

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

const lyricsSourceLabel = computed(() => {
  switch (props.lyrics?.source) {
    case "embedded":
      return "Embedded";
    case "sidecar":
      return "Local file";
    case "network": {
      const provider = props.lyrics?.provider?.trim();
      return provider?.replace(/\s+#.*$/, "") || "Network";
    }
    default:
      return null;
  }
});

watch(
  () => props.lyrics,
  async () => {
    await nextTick();
    if (lyricsViewport.value) {
      lyricsViewport.value.scrollTop = 0;
    }
  },
);

watch(activeLyricIndex, async (index) => {
  if (index < 0) {
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
    aria-label="Now playing details"
  >
    <div class="grid shrink-0 place-items-center border-b border-base-300 bg-base-200 p-4">
      <img
        v-if="coverUrl"
        class="aspect-square w-44 max-w-full rounded object-cover shadow-sm"
        :src="coverUrl"
        :alt="`${title} cover`"
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
      <div class="mt-0.5 truncate text-xs text-base-content/60" :title="subtitle">
        {{ subtitle }}
      </div>
    </div>

    <div class="flex min-h-0 flex-1 flex-col">
      <div class="flex h-11 shrink-0 items-center gap-2 border-b border-base-300 px-3">
        <FileText :size="16" aria-hidden="true" />
        <h2 class="min-w-0 flex-1 text-sm font-semibold">Lyrics</h2>
        <span
          v-if="lyricsSourceLabel"
          class="badge badge-ghost badge-sm max-w-24 truncate"
          :title="lyrics?.provider || lyricsSourceLabel"
        >
          {{ lyricsSourceLabel }}
        </span>
        <div v-if="canRetry" class="tooltip tooltip-left" data-tip="Retry lyrics">
          <button
            class="btn btn-square btn-ghost btn-xs"
            type="button"
            :disabled="lyricsLoading"
            aria-label="Retry lyrics"
            title="Retry lyrics"
            @click="emit('retryLyrics')"
          >
            <RefreshCw
              :class="{ 'animate-spin': lyricsLoading }"
              :size="14"
              aria-hidden="true"
            />
          </button>
        </div>
      </div>

      <div
        ref="lyricsViewport"
        class="relative h-64 min-h-0 flex-1 overflow-y-auto scroll-smooth px-4 text-center"
        data-testid="lyrics-viewport"
        aria-live="polite"
      >
        <div
          v-if="lyricsLoading"
          class="flex h-full min-h-48 items-center justify-center gap-2 text-sm text-base-content/55"
          role="status"
        >
          <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
          Loading lyrics
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
          class="grid h-full min-h-48 place-items-center py-8 text-sm text-base-content/45"
        >
          No lyrics available
        </div>

        <div v-else class="min-h-full py-20" data-testid="lyric-lines">
          <p
            v-for="(line, index) in lyrics.lines"
            :key="`${line.startMs ?? 'plain'}-${index}`"
            class="whitespace-pre-line py-2 text-sm leading-6 transition-colors duration-200"
            :class="
              index === activeLyricIndex
                ? 'font-semibold text-base-content'
                : 'text-base-content/45'
            "
            :data-lyric-index="index"
            :data-active="index === activeLyricIndex || undefined"
          >
            {{ line.text }}
          </p>
        </div>
      </div>
    </div>
  </section>
</template>
