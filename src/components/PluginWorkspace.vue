<script setup lang="ts">
import { computed } from "vue";
import { AudioLines, Plug, Settings } from "@lucide/vue";
import type { PluginRecord } from "../lib/plugin-api";

const props = defineProps<{
  plugin: PluginRecord;
}>();

const emit = defineEmits<{
  openPlugins: [];
}>();

const sourceCount = computed(() =>
  props.plugin.providers.reduce((count, provider) => count + provider.sources.length, 0),
);

function actionLabel(action: string) {
  const labels: Record<string, string> = {
    musicSearch: "Search",
    musicUrl: "Playback",
    lyric: "Lyrics",
    pic: "Artwork",
    musicRecommendations: "Recommendations",
    playlistList: "Playlists",
    playlistRead: "Playlist details",
    playlistAddTrack: "Add to playlists",
    playlistRemoveTrack: "Remove from playlists",
  };
  return labels[action] ?? action;
}
</script>

<template>
  <section
    class="overflow-hidden rounded border border-base-300 bg-base-100"
    data-testid="plugin-workspace"
  >
    <header
      class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
    >
      <div class="flex min-w-0 items-center gap-3">
        <div
          class="flex size-10 shrink-0 items-center justify-center rounded bg-neutral text-neutral-content"
        >
          <Plug :size="19" aria-hidden="true" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h2 class="truncate text-base font-semibold">{{ plugin.name }}</h2>
            <span class="badge badge-success badge-sm">Enabled</span>
            <span v-if="plugin.version" class="badge badge-outline badge-sm">
              v{{ plugin.version }}
            </span>
          </div>
          <p class="mt-0.5 truncate text-xs text-base-content/60">
            {{ plugin.author || plugin.id }}
          </p>
        </div>
      </div>

      <button class="btn btn-sm shrink-0" type="button" @click="emit('openPlugins')">
        <Settings :size="16" aria-hidden="true" />
        Manage plugin
      </button>
    </header>

    <p v-if="plugin.description" class="border-b border-base-300 px-4 py-3 text-sm text-base-content/70">
      {{ plugin.description }}
    </p>

    <div class="flex items-center justify-between gap-3 border-b border-base-300 px-4 py-3">
      <h3 class="text-sm font-semibold">Source providers</h3>
      <span class="text-xs text-base-content/60">
        {{ plugin.providers.length }} provider{{ plugin.providers.length === 1 ? "" : "s" }} ·
        {{ sourceCount }} source{{ sourceCount === 1 ? "" : "s" }}
      </span>
    </div>

    <ul v-if="plugin.providers.length" class="list divide-y divide-base-300">
      <li v-for="provider in plugin.providers" :key="provider.id" class="list-row px-4 py-4">
        <div class="flex size-9 shrink-0 items-center justify-center rounded bg-base-200">
          <AudioLines :size="17" aria-hidden="true" />
        </div>

        <div class="list-col-grow min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h4 class="font-medium">{{ provider.id }}</h4>
            <span
              class="badge badge-sm"
              :class="provider.initialized ? 'badge-success' : 'badge-warning'"
            >
              {{ provider.initialized ? "Initialized" : "Unavailable" }}
            </span>
          </div>
          <p class="mt-1 truncate text-xs text-base-content/55" :title="provider.entrypoint">
            {{ provider.entrypoint }}
          </p>

          <ul v-if="provider.sources.length" class="mt-3 divide-y divide-base-300 rounded border border-base-300">
            <li
              v-for="source in provider.sources"
              :key="source.id"
              class="flex flex-col gap-2 px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between"
            >
              <div class="min-w-0">
                <div class="truncate text-sm font-medium">{{ source.name }}</div>
                <div class="truncate text-xs text-base-content/55">{{ source.id }}</div>
              </div>
              <div class="flex flex-wrap gap-1 sm:justify-end">
                <span
                  v-for="action in source.actions"
                  :key="action"
                  class="badge badge-ghost badge-sm"
                >
                  {{ actionLabel(action) }}
                </span>
              </div>
            </li>
          </ul>
          <p v-else class="mt-3 text-xs text-base-content/55">No source catalog entries.</p>
        </div>
      </li>
    </ul>

    <div v-else class="p-6 text-sm text-base-content/60">No providers are registered.</div>

    <footer class="flex flex-wrap gap-x-4 gap-y-1 border-t border-base-300 px-4 py-3 text-xs text-base-content/55">
      <span>{{ plugin.id }}</span>
      <span>{{ plugin.origin === "bundled" ? "Bundled" : "User installed" }}</span>
    </footer>
  </section>
</template>
