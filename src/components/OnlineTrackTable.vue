<script setup lang="ts">
import { Download, Heart, ListPlus, Music2, Pause, Play, RefreshCw } from "@lucide/vue";
import type { OnlineTrack } from "../lib/online-music-api";

defineProps<{
  tracks: OnlineTrack[];
  activeKey: string | null;
  playing: boolean;
  resolvingKey: string | null;
  trackActionId: string | null;
  supportsLibraryActions: (track: OnlineTrack) => boolean;
  isFavorite: (track: OnlineTrack) => boolean;
}>();

defineEmits<{
  play: [track: OnlineTrack];
  download: [track: OnlineTrack];
  favorite: [track: OnlineTrack];
  addToPlaylist: [track: OnlineTrack];
}>();

function duration(seconds: number | null) {
  if (seconds === null) return "--:--";
  return `${Math.floor(seconds / 60)}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
}
</script>

<template>
  <div class="overflow-x-auto">
    <table class="table table-xs table-pin-rows">
      <thead>
        <tr>
          <th class="w-12"></th>
          <th>Title</th>
          <th class="hidden md:table-cell">Artist</th>
          <th class="hidden lg:table-cell">Album</th>
          <th class="w-28">Sources</th>
          <th class="w-16 text-right">Time</th>
          <th class="w-28"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="track in tracks" :key="track.key" :class="{ 'bg-base-200': activeKey === track.key }">
          <td>
            <button class="btn btn-square btn-ghost btn-xs" type="button" :aria-label="`Play ${track.title}`" @click="$emit('play', track)">
              <RefreshCw v-if="resolvingKey === track.key" class="animate-spin" :size="14" aria-hidden="true" />
              <Pause v-else-if="activeKey === track.key && playing" :size="14" aria-hidden="true" />
              <Play v-else :size="14" aria-hidden="true" />
            </button>
          </td>
          <td class="max-w-56">
            <div class="flex min-w-0 items-center gap-2">
              <div class="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
                <img v-if="track.coverUrl" :src="track.coverUrl" class="size-full object-cover" alt="" />
                <Music2 v-else :size="15" aria-hidden="true" />
              </div>
              <span class="truncate text-sm font-medium">{{ track.title }}</span>
            </div>
          </td>
          <td class="hidden max-w-44 truncate text-xs md:table-cell">{{ track.artist }}</td>
          <td class="hidden max-w-48 truncate text-xs text-base-content/60 lg:table-cell">{{ track.album || "-" }}</td>
          <td>
            <div class="flex max-w-28 gap-1 overflow-hidden">
              <span
                v-for="candidate in track.candidates"
                :key="candidate.channelId"
                class="badge badge-ghost badge-xs shrink-0"
                :title="candidate.channelName"
              >
                {{ candidate.sourceId.toUpperCase() }}
              </span>
            </div>
          </td>
          <td class="text-right text-xs tabular-nums text-base-content/55">{{ duration(track.durationSeconds) }}</td>
          <td>
            <div class="flex justify-end gap-1">
              <button
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="!supportsLibraryActions(track) || trackActionId === `favorite:${track.key}`"
                :aria-label="`Add ${track.title} to My Favorite Music`"
                :aria-pressed="isFavorite(track)"
                :title="supportsLibraryActions(track) ? 'Add to My Favorite Music' : 'This track is not available on NetEase or KuGou'"
                @click="$emit('favorite', track)"
              >
                <RefreshCw v-if="trackActionId === `favorite:${track.key}`" class="animate-spin" :size="14" aria-hidden="true" />
                <Heart
                  v-else
                  :class="{ 'text-error': isFavorite(track) }"
                  :fill="isFavorite(track) ? 'currentColor' : 'none'"
                  :size="14"
                  aria-hidden="true"
                />
              </button>
              <button
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="!supportsLibraryActions(track) || trackActionId === `playlist:${track.key}`"
                :aria-label="`Add ${track.title} to a Playlist`"
                :title="supportsLibraryActions(track) ? 'Add to Playlist' : 'This track is not available on NetEase or KuGou'"
                @click="$emit('addToPlaylist', track)"
              >
                <RefreshCw v-if="trackActionId === `playlist:${track.key}`" class="animate-spin" :size="14" aria-hidden="true" />
                <ListPlus v-else :size="14" aria-hidden="true" />
              </button>
              <button class="btn btn-square btn-ghost btn-xs" type="button" :aria-label="`Download ${track.title}`" title="Download" @click="$emit('download', track)">
                <Download :size="14" aria-hidden="true" />
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
