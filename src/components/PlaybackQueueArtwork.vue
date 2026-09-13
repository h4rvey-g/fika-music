<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { ListMusic } from "@lucide/vue";
import { TAURI_COMMANDS } from "../generated/bindings";
import type { PlaybackQueueItem } from "../lib/playback-queue";

const props = defineProps<{
  item: PlaybackQueueItem;
}>();

const root = ref<HTMLElement | null>(null);
const localCoverUrl = ref<string | null>(null);
const coverUrl = computed(() => props.item.kind === "online"
  ? props.item.track.coverUrl
  : localCoverUrl.value);
let observer: IntersectionObserver | null = null;

async function loadLocalCover() {
  if (props.item.kind !== "local" || localCoverUrl.value) return;
  try {
    localCoverUrl.value = await invoke<string | null>(
      TAURI_COMMANDS.localTrackCoverDataUrl,
      { trackId: props.item.track.id },
    );
  } catch {
    localCoverUrl.value = null;
  }
}

onMounted(() => {
  if (props.item.kind !== "local") return;
  if (typeof IntersectionObserver === "undefined") {
    void loadLocalCover();
    return;
  }

  observer = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    observer?.disconnect();
    observer = null;
    void loadLocalCover();
  }, { rootMargin: "96px" });
  if (root.value) observer.observe(root.value);
});

onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div
    ref="root"
    data-playback-queue-artwork
    class="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200"
  >
    <img
      v-if="coverUrl"
      class="size-full object-cover"
      :src="coverUrl"
      alt=""
      decoding="async"
    />
    <ListMusic v-else :size="16" aria-hidden="true" />
  </div>
</template>
