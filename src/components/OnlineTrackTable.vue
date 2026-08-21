<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  Download,
  FolderPlus,
  Heart,
  ListPlus,
  MessageCircle,
  Music2,
  Pause,
  RefreshCw,
  Volume2,
} from "@lucide/vue";
import {
  onlineTracksMatch,
  splitOnlineArtistNames,
  type OnlineTrack,
} from "../lib/online-music-api";
import { writeCollectionDragPayload } from "../lib/collection-api";
import { formatNumber, t } from "../i18n";

const props = defineProps<{
  tracks: OnlineTrack[];
  activeTrack: OnlineTrack | null;
  isPlaying: boolean;
  trackActionId: string | null;
  entityActionId?: string | null;
  supportsLibraryActions: (track: OnlineTrack) => boolean;
  supportsPlaylistSelection: (tracks: OnlineTrack[]) => boolean;
  supportsComments: (track: OnlineTrack) => boolean;
  isFavorite: (track: OnlineTrack) => boolean;
}>();

const emit = defineEmits<{
  play: [track: OnlineTrack];
  queueTracks: [tracks: OnlineTrack[], placement: "next" | "last"];
  download: [track: OnlineTrack, artwork: HTMLElement | null];
  downloadSelection: [tracks: OnlineTrack[]];
  favorite: [track: OnlineTrack];
  addToPlaylist: [track: OnlineTrack];
  addSelectionToPlaylist: [tracks: OnlineTrack[]];
  addToCollection: [tracks: OnlineTrack[]];
  createCollection: [tracks: OnlineTrack[]];
  viewComments: [track: OnlineTrack];
  openArtist: [track: OnlineTrack, artist: string];
  openAlbum: [track: OnlineTrack];
}>();

const selectedKeys = ref<Set<string>>(new Set());
const selectionAnchor = ref<number | null>(null);
const contextMenu = ref<{ x: number; y: number } | null>(null);
const selectedTracks = computed(() =>
  props.tracks.filter((track) => selectedKeys.value.has(track.key))
);
const selectionSupportsPlaylist = computed(() =>
  props.supportsPlaylistSelection(selectedTracks.value)
);
const selectionSupportsComments = computed(() => {
  const [track] = selectedTracks.value;
  return selectedTracks.value.length === 1 && Boolean(track && props.supportsComments(track));
});

watch(
  () => props.tracks,
  (tracks) => {
    const availableKeys = new Set(tracks.map((track) => track.key));
    selectedKeys.value = new Set(
      [...selectedKeys.value].filter((key) => availableKeys.has(key)),
    );
    if (selectionAnchor.value !== null && selectionAnchor.value >= tracks.length) {
      selectionAnchor.value = null;
    }
    closeContextMenu();
  },
);

onMounted(() => {
  window.addEventListener("pointerdown", handleWindowPointerDown);
  window.addEventListener("blur", closeContextMenu);
  window.addEventListener("resize", closeContextMenu);
  window.addEventListener("scroll", closeContextMenu, true);
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", handleWindowPointerDown);
  window.removeEventListener("blur", closeContextMenu);
  window.removeEventListener("resize", closeContextMenu);
  window.removeEventListener("scroll", closeContextMenu, true);
});

function selectTrack(event: MouseEvent, index: number) {
  const track = props.tracks[index];
  if (!track) return;

  if (event.shiftKey && selectionAnchor.value !== null) {
    const start = Math.min(selectionAnchor.value, index);
    const end = Math.max(selectionAnchor.value, index);
    const rangeKeys = props.tracks.slice(start, end + 1).map((item) => item.key);
    selectedKeys.value = event.metaKey || event.ctrlKey
      ? new Set([...selectedKeys.value, ...rangeKeys])
      : new Set(rangeKeys);
  } else if (event.metaKey || event.ctrlKey) {
    const next = new Set(selectedKeys.value);
    if (next.has(track.key)) next.delete(track.key);
    else next.add(track.key);
    selectedKeys.value = next;
    selectionAnchor.value = index;
  } else {
    selectedKeys.value = new Set([track.key]);
    selectionAnchor.value = index;
  }
}

function isActiveTrack(track: OnlineTrack) {
  return props.activeTrack !== null && onlineTracksMatch(track, props.activeTrack);
}

function openContextMenu(event: MouseEvent, index: number) {
  const track = props.tracks[index];
  if (!track) return;
  if (!selectedKeys.value.has(track.key)) {
    selectedKeys.value = new Set([track.key]);
    selectionAnchor.value = index;
  }
  contextMenu.value = menuPosition(event.clientX, event.clientY);
}

function closeContextMenu() {
  contextMenu.value = null;
}

function handleWindowPointerDown(event: PointerEvent) {
  const target = event.target;
  if (target instanceof Element && target.closest("[data-online-track-menu]")) return;
  closeContextMenu();
}

function downloadSelection() {
  if (selectedTracks.value.length) emit("downloadSelection", [...selectedTracks.value]);
  closeContextMenu();
}

function queueSelection(placement: "next" | "last") {
  if (selectedTracks.value.length) {
    emit("queueTracks", [...selectedTracks.value], placement);
  }
  closeContextMenu();
}

function downloadTrack(event: MouseEvent, track: OnlineTrack) {
  const trigger = event.currentTarget;
  const artwork = trigger instanceof HTMLElement
    ? trigger.closest("tr")?.querySelector<HTMLElement>("[data-online-track-artwork]") ?? null
    : null;
  emit("download", track, artwork);
}

function addSelectionToPlaylist() {
  if (selectedTracks.value.length) {
    emit("addSelectionToPlaylist", [...selectedTracks.value]);
  }
  closeContextMenu();
}

function requestCollectionAction(createNew: boolean) {
  if (selectedTracks.value.length) {
    const tracks = [...selectedTracks.value];
    if (createNew) emit("createCollection", tracks);
    else emit("addToCollection", tracks);
  }
  closeContextMenu();
}

function beginTrackDrag(event: DragEvent, index: number) {
  const track = props.tracks[index];
  if (!track) return;
  if (!selectedKeys.value.has(track.key)) {
    selectedKeys.value = new Set([track.key]);
    selectionAnchor.value = index;
  }
  writeCollectionDragPayload(event.dataTransfer, {
    kind: "online",
    tracks: [...selectedTracks.value],
  });
  closeContextMenu();
}

function viewComments() {
  const [track] = selectedTracks.value;
  if (selectionSupportsComments.value && track) emit("viewComments", track);
  closeContextMenu();
}

function menuPosition(x: number, y: number) {
  const width = 240;
  const height = 340;
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  };
}

function duration(seconds: number | null) {
  if (seconds === null) return "--:--";
  return `${Math.floor(seconds / 60)}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
}

function artistActionId(track: OnlineTrack, artist: string) {
  return `artist:${track.key}:${artist}`;
}
</script>

<template>
  <div>
    <div class="overflow-x-auto">
      <table class="table table-xs table-pin-rows">
        <thead>
          <tr>
            <th>{{ t("Title") }}</th>
            <th class="hidden md:table-cell">{{ t("Artist") }}</th>
            <th class="hidden lg:table-cell">{{ t("Album") }}</th>
            <th class="w-28">{{ t("Sources") }}</th>
            <th class="w-16 text-right">{{ t("Time") }}</th>
            <th class="w-28"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(track, index) in tracks"
            :key="track.key"
            :data-online-track-key="track.key"
            class="cursor-default select-none border-l-2 border-l-transparent"
            :class="[
              selectedKeys.has(track.key)
                ? 'bg-neutral text-neutral-content'
                : isActiveTrack(track)
                  ? 'bg-primary/10 hover:bg-primary/15'
                  : 'hover:bg-base-200/60',
              isActiveTrack(track)
                ? selectedKeys.has(track.key)
                  ? 'border-l-primary ring-1 ring-inset ring-primary/40'
                  : 'border-l-primary'
                : '',
            ]"
            :aria-selected="selectedKeys.has(track.key)"
            :aria-current="isActiveTrack(track) ? 'true' : undefined"
            :data-playing-track="isActiveTrack(track) ? '' : undefined"
            tabindex="0"
            draggable="true"
            @click="selectTrack($event, index)"
            @dblclick="$emit('play', track)"
            @keydown.enter.prevent="$emit('play', track)"
            @contextmenu.prevent.stop="openContextMenu($event, index)"
            @dragstart="beginTrackDrag($event, index)"
          >
          <td class="max-w-56">
            <div class="flex min-w-0 items-center gap-2">
              <span class="grid size-4 shrink-0 place-items-center">
                <Volume2
                  v-if="isActiveTrack(track) && isPlaying"
                  :class="selectedKeys.has(track.key) ? 'text-neutral-content' : 'text-primary'"
                  :size="13"
                  :aria-label="t('Playing')"
                />
                <Pause
                  v-else-if="isActiveTrack(track)"
                  :class="selectedKeys.has(track.key) ? 'text-neutral-content' : 'text-primary'"
                  :size="13"
                  :aria-label="t('Paused')"
                />
              </span>
              <div
                data-online-track-artwork
                class="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded"
                :class="selectedKeys.has(track.key) ? 'bg-neutral-content/15' : 'bg-base-200'"
              >
                <img v-if="track.coverUrl" :src="track.coverUrl" class="size-full object-cover" alt="" />
                <Music2 v-else :size="15" aria-hidden="true" />
              </div>
              <span class="truncate text-sm font-medium">{{ track.title }}</span>
            </div>
          </td>
          <td class="hidden max-w-44 text-xs md:table-cell">
            <div class="flex min-w-0 max-w-full flex-wrap items-center gap-x-1">
              <template
                v-for="(artist, artistIndex) in splitOnlineArtistNames(track.artist)"
                :key="`${track.key}:${artist}`"
              >
                <span v-if="artistIndex > 0" class="opacity-60" aria-hidden="true">/</span>
                <button
                  class="link link-hover inline-flex min-w-0 max-w-full items-center gap-1 text-left disabled:cursor-wait"
                  type="button"
                  :disabled="entityActionId === artistActionId(track, artist)"
                  :aria-busy="entityActionId === artistActionId(track, artist) ? 'true' : undefined"
                  :aria-label="t('Open artist {artist}', { artist })"
                  :data-online-track-artist="track.key"
                  :data-online-artist-name="artist"
                  @click.stop="$emit('openArtist', track, artist)"
                  @dblclick.stop
                  @keydown.enter.stop
                >
                  <span class="truncate">{{ artist }}</span>
                  <RefreshCw
                    v-if="entityActionId === artistActionId(track, artist)"
                    class="shrink-0 animate-spin"
                    :size="11"
                    aria-hidden="true"
                  />
                </button>
              </template>
            </div>
          </td>
          <td class="hidden max-w-48 text-xs lg:table-cell">
            <button
              v-if="track.album"
              class="link link-hover inline-flex min-w-0 max-w-full items-center gap-1 text-left text-muted disabled:cursor-wait"
              type="button"
              :disabled="entityActionId === `album:${track.key}`"
              :aria-busy="entityActionId === `album:${track.key}` ? 'true' : undefined"
              :aria-label="t('Open album {album}', { album: track.album })"
              :data-online-track-album="track.key"
              @click.stop="$emit('openAlbum', track)"
              @dblclick.stop
              @keydown.enter.stop
            >
              <span class="truncate">{{ track.album }}</span>
              <RefreshCw
                v-if="entityActionId === `album:${track.key}`"
                class="shrink-0 animate-spin"
                :size="11"
                aria-hidden="true"
              />
            </button>
            <span v-else class="text-muted">-</span>
          </td>
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
          <td class="text-right text-xs tabular-nums text-muted">{{ duration(track.durationSeconds) }}</td>
          <td>
            <div class="flex justify-end gap-1">
              <button
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="!supportsLibraryActions(track) || trackActionId === `favorite:${track.key}`"
                :aria-label="t('Add {title} to My Favorite Music', { title: track.title })"
                :aria-pressed="isFavorite(track)"
                :title="supportsLibraryActions(track) ? t('Add to My Favorite Music') : t('This track is not available on NetEase or KuGou')"
                @click.stop="$emit('favorite', track)"
                @dblclick.stop
              >
                <RefreshCw v-if="trackActionId === `favorite:${track.key}`" class="animate-spin" :size="16" aria-hidden="true" />
                <Heart
                  v-else
                  :class="{ 'text-error': isFavorite(track) }"
                  :fill="isFavorite(track) ? 'currentColor' : 'none'"
                  :size="16"
                  aria-hidden="true"
                />
              </button>
              <button
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="!supportsLibraryActions(track) || trackActionId === `playlist:${track.key}`"
                :aria-label="t('Add {title} to a Playlist', { title: track.title })"
                :title="supportsLibraryActions(track) ? t('Add to Playlist') : t('This track is not available on NetEase or KuGou')"
                @click.stop="$emit('addToPlaylist', track)"
                @dblclick.stop
              >
                <RefreshCw v-if="trackActionId === `playlist:${track.key}`" class="animate-spin" :size="16" aria-hidden="true" />
                <ListPlus v-else :size="16" aria-hidden="true" />
              </button>
              <button
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :aria-label="t('Download {title}', { title: track.title })"
                :title="t('Download')"
                @click.stop="downloadTrack($event, track)"
                @dblclick.stop
              >
                <Download :size="16" aria-hidden="true" />
              </button>
            </div>
          </td>
          </tr>
        </tbody>
      </table>
    </div>

    <ul
      v-if="contextMenu"
      class="menu fixed z-50 w-60 border border-base-300 bg-base-100 p-2 text-base-content shadow-xl"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      data-online-track-menu
      :aria-label="t('Selected online track actions')"
    >
      <li class="menu-title px-3 py-1 text-xs">
        {{ t(selectedTracks.length === 1 ? "{count} track selected" : "{count} tracks selected", { count: formatNumber(selectedTracks.length) }) }}
      </li>
      <li>
        <button type="button" @click="downloadSelection">
          <Download :size="16" aria-hidden="true" />
          {{ t("Download") }}
        </button>
      </li>
      <li>
        <button type="button" @click="queueSelection('next')">
          <ListPlus :size="16" aria-hidden="true" />
          {{ t("Play next") }}
        </button>
      </li>
      <li>
        <button type="button" @click="queueSelection('last')">
          <Music2 :size="16" aria-hidden="true" />
          {{ t("Add to queue") }}
        </button>
      </li>
      <li>
        <button type="button" @click="queueSelection('next')">
          <ListPlus :size="16" aria-hidden="true" />
          {{ t("Play next") }}
        </button>
      </li>
      <li>
        <button type="button" @click="queueSelection('last')">
          <Music2 :size="16" aria-hidden="true" />
          {{ t("Add to queue") }}
        </button>
      </li>
      <li>
        <button
          type="button"
          :disabled="!selectionSupportsPlaylist"
          @click="addSelectionToPlaylist"
        >
        <ListPlus :size="16" aria-hidden="true" />
          {{ t("Add to Playlist") }}
        </button>
      </li>
      <li>
        <button type="button" @click="requestCollectionAction(false)">
          <ListPlus :size="16" aria-hidden="true" />
          {{ t("Add to Collection") }}
        </button>
      </li>
      <li>
        <button type="button" @click="requestCollectionAction(true)">
          <FolderPlus :size="16" aria-hidden="true" />
          {{ t("New Collection from selection") }}
        </button>
      </li>
      <li>
        <button
          type="button"
          :disabled="!selectionSupportsComments"
          :title="selectedTracks.length === 1 && !selectionSupportsComments
            ? t('This track has no NetEase or KuGou comment source')
            : undefined"
          @click="viewComments"
        >
          <MessageCircle :size="16" aria-hidden="true" />
          {{ t("View Comments") }}
        </button>
      </li>
    </ul>
  </div>
</template>
