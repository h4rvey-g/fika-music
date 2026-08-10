<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Clipboard,
  Disc3,
  Download,
  FolderPlus,
  FolderSearch,
  Info,
  ListFilter,
  ListMusic,
  ListPlus,
  LoaderCircle,
  Pause,
  Play,
  Radio,
  RefreshCw,
  RotateCcw,
  Search,
  SlidersHorizontal,
  Tags,
  Trash2,
  Volume2,
  X,
} from "@lucide/vue";
import {
  getMusicCollection,
  removeMusicCollectionItems,
  startMusicCollectionMetadataLookup,
  writeCollectionDragPayload,
  type CollectionItemSelection,
  type MusicCollectionDetail,
  type MusicCollectionItem,
  type MusicCollectionSummary,
} from "../lib/collection-api";
import {
  buildCollectionAlbumGroups,
  COLLECTION_COLUMN_DEFINITIONS,
  COLLECTION_SEARCH_FIELD_OPTIONS,
  displayCollectionTrackValue,
  formatCollectionDuration,
  formatCollectionLongDuration,
  type CollectionAlbumGroup,
  type CollectionTrackView,
} from "../lib/collection-browser-model";
import { normalizeError } from "../lib/errors";
import { formatNumber, t } from "../i18n";
import {
  LIBRARY_COLUMN_DEFAULTS,
  loadLibraryPreferences,
  saveLibraryPreferences,
  type LibraryColumnId,
  type LibraryColumnPreference,
} from "../lib/library-preferences";
import { onlineTracksMatch, type OnlineTrack } from "../lib/online-music-api";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  AlbumArtSettings,
  AlbumArtTaskStatus,
  AlbumCoverCandidate,
  AlbumCoverResult,
  LibraryChangedEvent,
  LibrarySortField,
  LibraryTextField,
  LocalTrack,
  MetadataLookupTaskStatus,
} from "../generated/bindings";
type MenuPosition = { x: number; y: number };
type RowMenu = MenuPosition & { track: CollectionTrackView };
type GroupMenu = MenuPosition & { group: CollectionAlbumGroup };

const props = defineProps<{
  collectionId: string;
  refreshKey: number;
  activeLocalTrackId: number | null;
  activeOnlineTrack: OnlineTrack | null;
  isPlaying: boolean;
}>();

const emit = defineEmits<{
  play: [items: MusicCollectionItem[], index: number, autoplay: boolean];
  addToCollection: [source: CollectionItemSelection];
  createCollection: [source: CollectionItemSelection];
  changed: [collection: MusicCollectionSummary];
  error: [message: string];
}>();

const savedPreferences = loadLibraryPreferences();
const columns = ref(savedPreferences.columns.map((column) => ({ ...column })));
const searchFields = ref([...savedPreferences.searchFields]);
const persistedSortField = ref(savedPreferences.sortField);
const persistedSortDirection = ref(savedPreferences.sortDirection);
const sortField = ref(savedPreferences.sortField);
const sortDirection = ref(savedPreferences.sortDirection);
const searchInput = ref("");
const searchViewport = ref<HTMLInputElement | null>(null);
const searchScopeMenu = ref<HTMLDetailsElement | null>(null);
const scrollViewport = ref<HTMLElement | null>(null);
const detail = ref<MusicCollectionDetail | null>(null);
const loading = ref(false);
const removing = ref(false);
const selectedItemIds = ref(new Set<string>());
const selectionAnchor = ref<number | null>(null);
const focusedItemId = ref<string | null>(null);
const collapsedGroupIds = ref(new Set<string>());
const rowMenu = ref<RowMenu | null>(null);
const groupMenu = ref<GroupMenu | null>(null);
const columnMenu = ref<MenuPosition | null>(null);
const propertiesTrack = ref<LocalTrack | null>(null);
const queueStatus = ref<string | null>(null);
const draggedColumn = ref<LibraryColumnId | null>(null);
const resizing = ref<{ id: LibraryColumnId; startX: number; startWidth: number } | null>(null);
const albumArtSettings = ref<AlbumArtSettings>({ networkEnabled: false });
const albumArtTask = ref<AlbumArtTaskStatus | null>(null);
const metadataTask = ref<MetadataLookupTaskStatus | null>(null);
const albumCovers = shallowRef(new Map<string, AlbumCoverResult>());
const networkPermissionOpen = ref(false);
const networkPermissionDismissed = ref(false);
const pendingNetworkAction = ref<"backfill" | "metadata" | null>(null);
const metadataConfirmOpen = ref(false);
const coverReview = ref<{
  group: CollectionAlbumGroup;
  candidates: AlbumCoverCandidate[];
  message: string | null;
} | null>(null);
const pendingCoverIds = new Set<string>();
const coverQueue: string[] = [];

let loadGeneration = 0;
let componentUnmounted = false;
let pendingCollectionLoad: { collectionId: string; promise: Promise<void> } | null = null;
let searchWasEmpty = true;
let temporaryRelevanceSort = false;
let queueStatusTimer: ReturnType<typeof setTimeout> | null = null;
let isPumpingCovers = false;
let unlistenAlbumArt: (() => void) | null = null;
let unlistenMetadata: (() => void) | null = null;
let unlistenLibraryChanged: (() => void) | null = null;

const rowHeight = 34;
const albumCoverSize = rowHeight * 2 - 8;
const visibleColumns = computed(() =>
  columns.value
    .filter((column) => column.visible)
    .map((column) => ({
      ...column,
      definition: COLLECTION_COLUMN_DEFINITIONS.find((definition) => definition.id === column.id)!,
    })),
);
const minimumTableWidth = computed(() =>
  visibleColumns.value.reduce((sum, column) => sum + column.width, 0),
);
const gridTemplateColumns = computed(() =>
  visibleColumns.value.map((column) => `${column.width}px`).join(" "),
);
const tableWidthStyle = computed(() => ({ width: `max(100%, ${minimumTableWidth.value}px)` }));
const tableGridStyle = computed(() => ({
  gridTemplateColumns: gridTemplateColumns.value,
  ...tableWidthStyle.value,
}));
const groups = computed(() => buildCollectionAlbumGroups(
  detail.value?.items ?? [],
  searchInput.value,
  searchFields.value,
  sortField.value,
  sortDirection.value,
));
const visibleTracks = computed(() => groups.value.flatMap((group) => group.tracks));
const selectionCount = computed(() => selectedItemIds.value.size);
const selectedTracks = computed(() =>
  visibleTracks.value.filter((track) => selectedItemIds.value.has(track.item.id)),
);
const selectedLocalItemIds = computed(() =>
  selectedTracks.value
    .filter((track) => track.item.localTrack)
    .map((track) => track.item.id),
);
const totalDurationSeconds = computed(() =>
  visibleTracks.value.reduce((total, track) => total + (track.durationSeconds ?? 0), 0),
);
const resultSummary = computed(() => {
  const total = detail.value?.collection.itemCount ?? 0;
  const visible = visibleTracks.value.length;
  const albums = t(groups.value.length === 1 ? "{count} album" : "{count} albums", {
    count: formatNumber(groups.value.length),
  });
  const duration = formatCollectionLongDuration(totalDurationSeconds.value);
  return searchInput.value.trim()
    ? t("{visible} of {total} tracks in {albums} - {duration}", {
        visible: formatNumber(visible),
        total: formatNumber(total),
        albums,
        duration,
      })
    : t("{total} tracks in {albums} - {duration}", {
        total: formatNumber(total),
        albums,
        duration,
      });
});
const activeSortLabel = computed(() => {
  if (sortField.value === "relevance") return t("Collection order");
  const label = COLLECTION_COLUMN_DEFINITIONS.find(
    (column) => column.sortField === sortField.value,
  )?.label;
  return label ? t(label) : t("Sort");
});
const albumTaskPercent = computed(() => taskPercent(albumArtTask.value));
const metadataTaskPercent = computed(() => taskPercent(metadataTask.value));
const isSmartCollection = computed(() => Boolean(detail.value?.collection.smartRules));

watch(
  () => [props.collectionId, props.refreshKey] as const,
  () => void loadCollection(),
);

watch(searchInput, (value) => {
  const isEmpty = value.trim().length === 0;
  if (searchWasEmpty && !isEmpty) {
    sortField.value = "relevance";
    sortDirection.value = "descending";
    temporaryRelevanceSort = true;
  } else if (!searchWasEmpty && isEmpty && temporaryRelevanceSort) {
    sortField.value = persistedSortField.value;
    sortDirection.value = persistedSortDirection.value;
    temporaryRelevanceSort = false;
  }
  searchWasEmpty = isEmpty;
  clearSelection();
});

watch(
  () => groups.value.map((group) => group.localAlbumGroupId).filter(Boolean).join(","),
  scheduleVisibleCovers,
);

watch(
  () => [props.activeLocalTrackId, props.activeOnlineTrack?.key] as const,
  () => void nextTick(scrollActiveTrackIntoView),
);

onMounted(() => {
  window.addEventListener("pointerdown", handleWindowPointerDown);
  window.addEventListener("keydown", handleWindowKeydown);
  void loadCollection();
  void initializeLibraryFeatures();
});

onBeforeUnmount(() => {
  componentUnmounted = true;
  loadGeneration += 1;
  window.removeEventListener("pointerdown", handleWindowPointerDown);
  window.removeEventListener("keydown", handleWindowKeydown);
  stopResize();
  unlistenAlbumArt?.();
  unlistenMetadata?.();
  unlistenLibraryChanged?.();
  if (queueStatusTimer) clearTimeout(queueStatusTimer);
});

function loadCollection() {
  const collectionId = props.collectionId;
  const generation = ++loadGeneration;
  loading.value = true;
  if (detail.value?.collection.id !== collectionId) {
    detail.value = null;
    clearSelection();
  }
  const promise = (async () => {
    try {
      const loaded = await getMusicCollection(collectionId);
      if (generation !== loadGeneration) return;
      detail.value = loaded;
      clearSelection();
      collapsedGroupIds.value = new Set(
        [...collapsedGroupIds.value].filter((groupId) =>
          groups.value.some((group) => group.id === groupId)),
      );
      await nextTick();
      scheduleVisibleCovers();
      scrollActiveTrackIntoView();
    } catch (error) {
      if (generation === loadGeneration) emit("error", normalizeError(error));
    } finally {
      if (generation === loadGeneration) loading.value = false;
    }
  })();
  pendingCollectionLoad = { collectionId, promise };
  return promise;
}

async function initializeLibraryFeatures() {
  try {
    const [settings, artStatus, lookupStatus] = await Promise.all([
      invoke<AlbumArtSettings>(TAURI_COMMANDS.getAlbumArtSettings),
      invoke<AlbumArtTaskStatus>(TAURI_COMMANDS.getAlbumArtTaskStatus),
      invoke<MetadataLookupTaskStatus>(TAURI_COMMANDS.getMetadataLookupTaskStatus),
    ]);
    albumArtSettings.value = settings;
    albumArtTask.value = artStatus;
    metadataTask.value = lookupStatus;
  } catch (error) {
    emit("error", normalizeError(error));
  }
  try {
    unlistenAlbumArt = await listen<AlbumArtTaskStatus>("library:album-art-progress", (event) => {
      const previous = albumArtTask.value?.state;
      albumArtTask.value = event.payload;
      if (event.payload.state === "completed" && previous !== "completed") {
        albumCovers.value = new Map();
        scheduleVisibleCovers();
      }
    });
    unlistenMetadata = await listen<MetadataLookupTaskStatus>(
      "library:metadata-lookup-progress",
      (event) => {
        const previous = metadataTask.value?.state;
        metadataTask.value = event.payload;
        if (
          (event.payload.state === "completed" || event.payload.state === "paused")
          && previous !== event.payload.state
        ) {
          void loadCollection();
        }
      },
    );
    unlistenLibraryChanged = await listen<LibraryChangedEvent>("library:changed", () => {
      void loadCollection();
    });
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function isActive(track: CollectionTrackView) {
  if (track.item.localTrack) return props.activeLocalTrackId === track.item.localTrack.id;
  return Boolean(
    track.item.onlineTrack
    && props.activeOnlineTrack
    && onlineTracksMatch(track.item.onlineTrack, props.activeOnlineTrack),
  );
}

function selectTrack(event: MouseEvent, track: CollectionTrackView) {
  const index = visibleTrackIndex(track.item.id);
  if (index < 0) return;
  focusedItemId.value = track.item.id;
  if (event.shiftKey && selectionAnchor.value !== null) {
    selectRange(selectionAnchor.value, index, event.metaKey || event.ctrlKey);
  } else if (event.metaKey || event.ctrlKey) {
    const next = new Set(selectedItemIds.value);
    if (next.has(track.item.id)) next.delete(track.item.id);
    else next.add(track.item.id);
    selectedItemIds.value = next;
    selectionAnchor.value = index;
  } else {
    selectedItemIds.value = new Set([track.item.id]);
    selectionAnchor.value = index;
  }
  scrollViewport.value?.focus({ preventScroll: true });
}

function selectGroup(event: MouseEvent, group: CollectionAlbumGroup) {
  const indexes = group.tracks
    .map((track) => visibleTrackIndex(track.item.id))
    .filter((index) => index >= 0);
  if (!indexes.length) return;
  const start = Math.min(...indexes);
  const end = Math.max(...indexes);
  focusedItemId.value = group.tracks[0].item.id;
  if (event.shiftKey && selectionAnchor.value !== null) {
    const endpoint = start < selectionAnchor.value ? start : end;
    selectRange(selectionAnchor.value, endpoint, event.metaKey || event.ctrlKey);
  } else if (event.metaKey || event.ctrlKey) {
    const next = new Set(selectedItemIds.value);
    const selected = isGroupSelected(group);
    for (const track of group.tracks) {
      if (selected) next.delete(track.item.id);
      else next.add(track.item.id);
    }
    selectedItemIds.value = next;
    selectionAnchor.value = start;
  } else {
    selectedItemIds.value = new Set(group.tracks.map((track) => track.item.id));
    selectionAnchor.value = start;
  }
  scrollViewport.value?.focus({ preventScroll: true });
}

function selectRange(first: number, second: number, additive: boolean) {
  const start = Math.min(first, second);
  const end = Math.max(first, second);
  const next = additive ? new Set(selectedItemIds.value) : new Set<string>();
  visibleTracks.value.slice(start, end + 1).forEach((track) => next.add(track.item.id));
  selectedItemIds.value = next;
}

function isGroupSelected(group: CollectionAlbumGroup) {
  return group.tracks.length > 0
    && group.tracks.every((track) => selectedItemIds.value.has(track.item.id));
}

function visibleTrackIndex(itemId: string) {
  return visibleTracks.value.findIndex((track) => track.item.id === itemId);
}

function clearSelection() {
  selectedItemIds.value = new Set();
  selectionAnchor.value = null;
  focusedItemId.value = null;
}

function openRowMenu(event: MouseEvent, track: CollectionTrackView) {
  event.preventDefault();
  if (!selectedItemIds.value.has(track.item.id)) {
    selectedItemIds.value = new Set([track.item.id]);
    selectionAnchor.value = visibleTrackIndex(track.item.id);
  }
  focusedItemId.value = track.item.id;
  groupMenu.value = null;
  columnMenu.value = null;
  rowMenu.value = { ...menuPosition(event.clientX, event.clientY, 250, 440), track };
}

function openGroupMenu(event: MouseEvent, group: CollectionAlbumGroup) {
  event.preventDefault();
  if (!isGroupSelected(group)) {
    selectedItemIds.value = new Set(group.tracks.map((track) => track.item.id));
    selectionAnchor.value = visibleTrackIndex(group.tracks[0]?.item.id ?? "");
  }
  rowMenu.value = null;
  columnMenu.value = null;
  groupMenu.value = { ...menuPosition(event.clientX, event.clientY, 260, 400), group };
}

function playVisibleTrack(track: CollectionTrackView) {
  emitQueue(visibleTracks.value, track.item.id, true);
}

function playSelection(autoplay: boolean) {
  const tracks = selectedTracks.value;
  if (!tracks.length) return;
  const startId = rowMenu.value?.track.item.id ?? tracks[0].item.id;
  emitQueue(tracks, startId, autoplay);
  showQueueStatus(t(tracks.length === 1 ? "{count} track queued" : "{count} tracks queued", {
    count: formatNumber(tracks.length),
  }));
  closeMenus();
}

function playGroup(group: CollectionAlbumGroup, autoplay: boolean) {
  if (!group.tracks.length) return;
  emitQueue(group.tracks, group.tracks[0].item.id, autoplay);
  showQueueStatus(t(group.tracks.length === 1 ? "{count} track queued" : "{count} tracks queued", {
    count: formatNumber(group.tracks.length),
  }));
  closeMenus();
}

function emitQueue(tracks: CollectionTrackView[], startItemId: string, autoplay: boolean) {
  const items = tracks.map((track) => track.item);
  const index = Math.max(0, tracks.findIndex((track) => track.item.id === startItemId));
  emit("play", items, index, autoplay);
}

function playCollection() {
  const tracks = selectedTracks.value.length ? selectedTracks.value : visibleTracks.value;
  if (tracks.length) emitQueue(tracks, tracks[0].item.id, true);
}

function requestCollectionAction(createNew: boolean) {
  if (!selectedItemIds.value.size) return;
  const source: CollectionItemSelection = {
    sourceCollectionId: props.collectionId,
    itemIds: selectedTracks.value.map((track) => track.item.id),
  };
  if (createNew) emit("createCollection", source);
  else emit("addToCollection", source);
  closeMenus();
}

function beginTrackDrag(event: DragEvent, track: CollectionTrackView) {
  if (!selectedItemIds.value.has(track.item.id)) {
    selectedItemIds.value = new Set([track.item.id]);
    selectionAnchor.value = visibleTrackIndex(track.item.id);
  }
  writeSelectedDragPayload(event.dataTransfer);
  closeMenus();
}

function beginGroupDrag(event: DragEvent, group: CollectionAlbumGroup) {
  if (!isGroupSelected(group)) {
    selectedItemIds.value = new Set(group.tracks.map((track) => track.item.id));
    selectionAnchor.value = visibleTrackIndex(group.tracks[0]?.item.id ?? "");
  }
  writeSelectedDragPayload(event.dataTransfer);
  closeMenus();
}

function writeSelectedDragPayload(dataTransfer: DataTransfer | null) {
  writeCollectionDragPayload(dataTransfer, {
    kind: "collection",
    sourceCollectionId: props.collectionId,
    itemIds: selectedTracks.value.map((track) => track.item.id),
  });
}

async function removeSelection() {
  const itemIds = selectedTracks.value.map((track) => track.item.id);
  if (!detail.value || removing.value || !itemIds.length) return;
  removing.value = true;
  closeMenus();
  try {
    const mutation = await removeMusicCollectionItems(props.collectionId, itemIds);
    const removed = new Set(itemIds);
    detail.value = {
      collection: mutation.collection,
      items: detail.value.items.filter((item) => !removed.has(item.id)),
    };
    clearSelection();
    emit("changed", mutation.collection);
    showQueueStatus(t(mutation.removed === 1 ? "{count} track removed" : "{count} tracks removed", {
      count: formatNumber(mutation.removed),
    }));
  } catch (error) {
    emit("error", normalizeError(error));
  } finally {
    removing.value = false;
  }
}

async function revealContextTrack() {
  const track = rowMenu.value?.track.item.localTrack;
  if (!track) return;
  try {
    await revealItemInDir(track.filePath);
    rowMenu.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function copyContextPath() {
  const track = rowMenu.value?.track.item.localTrack;
  if (!track) return;
  try {
    await navigator.clipboard.writeText(track.filePath);
    showQueueStatus(t("Path copied"));
    rowMenu.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function showProperties() {
  propertiesTrack.value = rowMenu.value?.track.item.localTrack ?? null;
  rowMenu.value = null;
}

function toggleGroup(group: CollectionAlbumGroup) {
  const next = new Set(collapsedGroupIds.value);
  if (next.has(group.id)) next.delete(group.id);
  else next.add(group.id);
  collapsedGroupIds.value = next;
  closeMenus();
}

function clickSort(sort: LibrarySortField | null) {
  if (!sort || draggedColumn.value || resizing.value) return;
  if (sortField.value === sort) {
    sortDirection.value = sortDirection.value === "ascending" ? "descending" : "ascending";
  } else {
    sortField.value = sort;
    sortDirection.value = "ascending";
  }
  persistedSortField.value = sortField.value;
  persistedSortDirection.value = sortDirection.value;
  temporaryRelevanceSort = false;
  persistPreferences();
  clearSelection();
}

function restoreCollectionOrder() {
  sortField.value = "relevance";
  sortDirection.value = "descending";
  temporaryRelevanceSort = true;
  clearSelection();
}

function toggleSearchField(field: LibraryTextField) {
  if (searchFields.value.includes(field)) {
    if (searchFields.value.length === 1) return;
    searchFields.value = searchFields.value.filter((candidate) => candidate !== field);
  } else {
    searchFields.value = [...searchFields.value, field];
  }
  persistPreferences();
  clearSelection();
}

function clearSearch() {
  searchInput.value = "";
  searchViewport.value?.focus();
}

function openColumnMenu(event: MouseEvent) {
  if (event.target instanceof Element && event.target.closest("[data-track-row], [data-album-row]")) {
    return;
  }
  event.preventDefault();
  rowMenu.value = null;
  groupMenu.value = null;
  columnMenu.value = menuPosition(event.clientX, event.clientY, 260, 520);
}

function toggleColumn(columnId: LibraryColumnId) {
  const target = columns.value.find((column) => column.id === columnId);
  if (!target) return;
  const visibleDataColumns = columns.value.filter(
    (column) => column.visible && column.id !== "playing",
  ).length;
  if (target.visible && target.id !== "playing" && visibleDataColumns === 1) return;
  target.visible = !target.visible;
  columns.value = [...columns.value];
  persistPreferences();
}

function resetColumns() {
  columns.value = LIBRARY_COLUMN_DEFAULTS.map((column) => ({ ...column }));
  persistPreferences();
  columnMenu.value = null;
}

function beginColumnDrag(event: DragEvent, id: LibraryColumnId) {
  draggedColumn.value = id;
  event.dataTransfer?.setData("text/plain", id);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

function dropColumn(event: DragEvent, targetId: LibraryColumnId) {
  event.preventDefault();
  const sourceId = draggedColumn.value;
  draggedColumn.value = null;
  if (!sourceId || sourceId === targetId) return;
  const next = [...columns.value];
  const sourceIndex = next.findIndex((column) => column.id === sourceId);
  const targetIndex = next.findIndex((column) => column.id === targetId);
  if (sourceIndex < 0 || targetIndex < 0) return;
  const [source] = next.splice(sourceIndex, 1);
  next.splice(targetIndex, 0, source);
  columns.value = next;
  persistPreferences();
}

function finishColumnDrag() {
  window.setTimeout(() => {
    draggedColumn.value = null;
  }, 0);
}

function beginResize(event: PointerEvent, column: LibraryColumnPreference) {
  event.preventDefault();
  event.stopPropagation();
  resizing.value = { id: column.id, startX: event.clientX, startWidth: column.width };
  window.addEventListener("pointermove", resizeColumn);
  window.addEventListener("pointerup", finishResize, { once: true });
}

function resizeColumn(event: PointerEvent) {
  const state = resizing.value;
  if (!state) return;
  const column = columns.value.find((candidate) => candidate.id === state.id);
  if (!column) return;
  column.width = Math.min(640, Math.max(36, state.startWidth + event.clientX - state.startX));
  columns.value = [...columns.value];
}

function finishResize() {
  if (resizing.value) persistPreferences();
  stopResize();
}

function stopResize() {
  resizing.value = null;
  window.removeEventListener("pointermove", resizeColumn);
  window.removeEventListener("pointerup", finishResize);
}

function autoFitColumn(columnId: LibraryColumnId) {
  const column = columns.value.find((candidate) => candidate.id === columnId);
  const definition = COLLECTION_COLUMN_DEFINITIONS.find((candidate) => candidate.id === columnId);
  if (!column || !definition) return;
  const widest = [
    definition.label,
    ...visibleTracks.value.map((track) => displayCollectionTrackValue(track, columnId)),
  ].reduce((maximum, value) => Math.max(maximum, visualTextWidth(value)), 0);
  column.width = Math.min(640, Math.max(columnId === "playing" ? 36 : 56, widest + 28));
  columns.value = [...columns.value];
  persistPreferences();
}

function persistPreferences() {
  saveLibraryPreferences({
    columns: columns.value.map((column) => ({ ...column })),
    searchFields: [...searchFields.value],
    sortField: persistedSortField.value,
    sortDirection: persistedSortDirection.value,
  });
}

function requestAlbumBackfill() {
  if (albumArtTask.value?.state === "paused") {
    void resumeAlbumBackfill();
    return;
  }
  if (!albumArtSettings.value.networkEnabled) {
    pendingNetworkAction.value = "backfill";
    networkPermissionOpen.value = true;
    return;
  }
  void startAlbumBackfill();
}

async function startAlbumBackfill() {
  try {
    albumArtTask.value = await invoke<AlbumArtTaskStatus>(TAURI_COMMANDS.startAlbumArtBackfill);
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function pauseAlbumBackfill() {
  try {
    albumArtTask.value = await invoke<AlbumArtTaskStatus>(TAURI_COMMANDS.pauseAlbumArtBackfill);
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function resumeAlbumBackfill() {
  try {
    albumArtTask.value = await invoke<AlbumArtTaskStatus>(TAURI_COMMANDS.resumeAlbumArtBackfill);
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function requestMetadataLookup() {
  closeMenus();
  if (!selectedLocalItemIds.value.length) return;
  if (!albumArtSettings.value.networkEnabled) {
    pendingNetworkAction.value = "metadata";
    networkPermissionOpen.value = true;
    return;
  }
  if (selectedLocalItemIds.value.length > 1) metadataConfirmOpen.value = true;
  else void startMetadataLookup();
}

async function startMetadataLookup() {
  const itemIds = selectedLocalItemIds.value;
  if (!itemIds.length) return;
  metadataConfirmOpen.value = false;
  try {
    metadataTask.value = await startMusicCollectionMetadataLookup(props.collectionId, itemIds);
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function pauseMetadataLookup() {
  try {
    metadataTask.value = await invoke<MetadataLookupTaskStatus>(
      TAURI_COMMANDS.pauseLocalMetadataLookup,
    );
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function resumeMetadataLookup() {
  try {
    metadataTask.value = await invoke<MetadataLookupTaskStatus>(
      TAURI_COMMANDS.resumeLocalMetadataLookup,
    );
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function authorizeOnlineMetadata() {
  try {
    albumArtSettings.value = await invoke<AlbumArtSettings>(
      TAURI_COMMANDS.setAlbumArtNetworkEnabled,
      { enabled: true },
    );
    networkPermissionOpen.value = false;
    networkPermissionDismissed.value = false;
    const action = pendingNetworkAction.value;
    pendingNetworkAction.value = null;
    scheduleVisibleCovers();
    if (action === "backfill") await startAlbumBackfill();
    else if (action === "metadata") requestMetadataLookup();
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function dismissOnlineMetadata() {
  networkPermissionOpen.value = false;
  networkPermissionDismissed.value = true;
  pendingNetworkAction.value = null;
}

function scheduleVisibleCovers() {
  for (const group of groups.value) {
    const groupId = group.localAlbumGroupId;
    if (
      !groupId
      || albumCovers.value.has(groupId)
      || pendingCoverIds.has(groupId)
      || coverQueue.includes(groupId)
    ) {
      continue;
    }
    coverQueue.push(groupId);
  }
  void pumpCoverQueue();
}

async function pumpCoverQueue() {
  if (isPumpingCovers) return;
  isPumpingCovers = true;
  try {
    while (coverQueue.length) {
      const groupId = coverQueue.shift();
      if (!groupId || albumCovers.value.has(groupId)) continue;
      pendingCoverIds.add(groupId);
      try {
        const result = await invoke<AlbumCoverResult>(TAURI_COMMANDS.resolveLocalAlbumCover, {
          groupId,
          releaseGroupId: null,
        });
        setAlbumCover(result);
        if (
          result.status === "authorizationRequired"
          && !networkPermissionDismissed.value
        ) {
          networkPermissionOpen.value = true;
        }
      } catch (error) {
        setAlbumCover({
          groupId,
          status: "failed",
          dataUrl: null,
          candidates: [],
          message: normalizeError(error),
          writtenTracks: 0,
          failedTracks: 0,
        });
      } finally {
        pendingCoverIds.delete(groupId);
      }
    }
  } finally {
    isPumpingCovers = false;
  }
}

function setAlbumCover(result: AlbumCoverResult) {
  const next = new Map(albumCovers.value);
  next.set(result.groupId, result);
  while (next.size > 96) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  albumCovers.value = next;
}

function coverResult(group: CollectionAlbumGroup) {
  return group.localAlbumGroupId ? albumCovers.value.get(group.localAlbumGroupId) : undefined;
}

function coverUrl(group: CollectionAlbumGroup) {
  return coverResult(group)?.dataUrl || group.coverUrl;
}

function reviewGroupCover(group: CollectionAlbumGroup) {
  const result = coverResult(group);
  if (!result?.candidates.length) return;
  coverReview.value = { group, candidates: result.candidates, message: result.message };
  closeMenus();
}

async function chooseCoverCandidate(candidate: AlbumCoverCandidate) {
  const review = coverReview.value;
  const groupId = review?.group.localAlbumGroupId;
  if (!review || !groupId) return;
  try {
    const result = await invoke<AlbumCoverResult>(TAURI_COMMANDS.resolveLocalAlbumCover, {
      groupId,
      releaseGroupId: candidate.releaseGroupId,
    });
    setAlbumCover(result);
    coverReview.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function handleGridKeydown(event: KeyboardEvent) {
  if (event.key.toLowerCase() === "a" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    selectedItemIds.value = new Set(visibleTracks.value.map((track) => track.item.id));
    selectionAnchor.value = visibleTracks.value.length ? 0 : null;
    focusedItemId.value = visibleTracks.value[0]?.item.id ?? null;
    return;
  }
  if (!visibleTracks.value.length) return;
  if (event.key === "Enter" && focusedItemId.value) {
    event.preventDefault();
    const focused = visibleTracks.value.find((track) => track.item.id === focusedItemId.value);
    if (focused) playVisibleTrack(focused);
    return;
  }
  const current = focusedItemId.value ? visibleTrackIndex(focusedItemId.value) : 0;
  let next: number | null = null;
  if (event.key === "ArrowDown") next = Math.min(visibleTracks.value.length - 1, current + 1);
  else if (event.key === "ArrowUp") next = Math.max(0, current - 1);
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = visibleTracks.value.length - 1;
  if (next === null) return;
  event.preventDefault();
  const track = visibleTracks.value[next];
  focusedItemId.value = track.item.id;
  if (event.shiftKey && selectionAnchor.value !== null) {
    selectRange(selectionAnchor.value, next, false);
  } else {
    selectedItemIds.value = new Set([track.item.id]);
    selectionAnchor.value = next;
  }
  await nextTick();
  document.getElementById(`collection-row-${track.item.id}`)?.scrollIntoView({ block: "nearest" });
}

function closeMenus() {
  rowMenu.value = null;
  groupMenu.value = null;
  columnMenu.value = null;
  if (searchScopeMenu.value) searchScopeMenu.value.open = false;
}

function handleWindowPointerDown(event: PointerEvent) {
  const target = event.target;
  if (target instanceof Element && target.closest("[data-menu-surface]")) return;
  closeMenus();
}

function handleWindowKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  closeMenus();
  propertiesTrack.value = null;
  coverReview.value = null;
  metadataConfirmOpen.value = false;
}

function scrollActiveTrackIntoView() {
  const track = visibleTracks.value.find(isActive);
  if (!track || collapsedGroupIds.value.has(groupForTrack(track)?.id ?? "")) return;
  document.getElementById(`collection-row-${track.item.id}`)?.scrollIntoView({ block: "center" });
}

function groupForTrack(track: CollectionTrackView) {
  return groups.value.find((group) => group.tracks.some((candidate) => candidate.item.id === track.item.id));
}

function showQueueStatus(message: string) {
  queueStatus.value = message;
  if (queueStatusTimer) clearTimeout(queueStatusTimer);
  queueStatusTimer = setTimeout(() => {
    queueStatus.value = null;
  }, 2_500);
}

function updatePlayCount(trackId: number, playCount: number) {
  if (!detail.value) return;
  detail.value = {
    ...detail.value,
    items: detail.value.items.map((item) => item.localTrack?.id === trackId
      ? { ...item, localTrack: { ...item.localTrack, playCount } }
      : item),
  };
}

async function waitForCollection(expectedCollectionId: string) {
  if (componentUnmounted || props.collectionId !== expectedCollectionId) return false;
  const pending = pendingCollectionLoad;
  if (pending?.collectionId === expectedCollectionId) {
    await pending.promise;
  } else if (detail.value?.collection.id !== expectedCollectionId) {
    await loadCollection();
  }
  if (
    componentUnmounted
    || props.collectionId !== expectedCollectionId
    || detail.value?.collection.id !== expectedCollectionId
  ) {
    return false;
  }
  return true;
}

async function startFirstTrack(expectedCollectionId = props.collectionId) {
  if (!await waitForCollection(expectedCollectionId)) return;
  const first = visibleTracks.value[0];
  if (first) emitQueue(visibleTracks.value, first.item.id, true);
}

async function startCollection(expectedCollectionId = props.collectionId) {
  if (!await waitForCollection(expectedCollectionId)) return;
  const items = detail.value?.items ?? [];
  if (items.length) emit("play", items, 0, true);
}

function displayValue(track: CollectionTrackView, columnId: LibraryColumnId) {
  return displayCollectionTrackValue(track, columnId);
}

function sortAria(sort: LibrarySortField | null) {
  if (!sort || sortField.value !== sort) return "none" as const;
  return sortDirection.value;
}

function menuPosition(x: number, y: number, width: number, height: number): MenuPosition {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  };
}

function visualTextWidth(value: string) {
  return [...value].reduce(
    (width, character) => width + (character.codePointAt(0)! > 0xff ? 14 : 7),
    0,
  );
}

function taskPercent(task: { total: number; processed: number } | null) {
  if (!task?.total) return 0;
  return Math.round((task.processed / task.total) * 100);
}

defineExpose({
  refresh: loadCollection,
  startCollection,
  startFirstTrack,
  updatePlayCount,
});
</script>

<template>
  <section
    class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded border border-base-300 bg-base-100"
    :aria-label="t('Collection library')"
  >
    <div class="flex shrink-0 items-center gap-2 border-b border-base-300 p-2">
      <label class="input input-sm min-w-0 flex-1" :aria-label="t('Search Collection')">
        <Search :size="15" aria-hidden="true" />
        <input
          ref="searchViewport"
          v-model="searchInput"
          class="min-w-0"
          type="text"
          role="searchbox"
          :aria-label="t('Search Collection')"
          autocomplete="off"
          spellcheck="false"
          :placeholder="t('Search title, artist, album')"
        />
        <button
          v-if="searchInput"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="t('Clear Collection search')"
          :title="t('Clear search')"
          @click="clearSearch"
        >
          <X :size="16" aria-hidden="true" />
        </button>
      </label>

      <details ref="searchScopeMenu" class="dropdown dropdown-end" data-menu-surface>
        <summary class="btn btn-sm" :title="t('Search fields')">
          <SlidersHorizontal :size="16" aria-hidden="true" />
          <span class="hidden xl:inline">{{ t("{count} fields", { count: searchFields.length }) }}</span>
        </summary>
        <ul class="menu dropdown-content z-50 mt-1 w-56 border border-base-300 bg-base-100 p-2 shadow-lg">
          <li class="menu-title">{{ t("Search fields") }}</li>
          <li v-for="field in COLLECTION_SEARCH_FIELD_OPTIONS" :key="field.id">
            <label class="flex cursor-pointer items-center gap-3">
              <input
                class="checkbox checkbox-md"
                type="checkbox"
                :checked="searchFields.includes(field.id)"
                :disabled="searchFields.includes(field.id) && searchFields.length === 1"
                @change="toggleSearchField(field.id)"
              />
              <span>{{ t(field.label) }}</span>
            </label>
          </li>
        </ul>
      </details>

      <button
        v-if="searchInput.trim() && sortField !== 'relevance'"
        class="btn btn-sm"
        type="button"
        :title="t('Restore Collection order')"
        @click="restoreCollectionOrder"
      >
        <ListFilter :size="16" aria-hidden="true" />
        <span class="hidden 2xl:inline">{{ t("Collection order") }}</span>
      </button>

      <div class="tooltip tooltip-left" :data-tip="t('Complete missing album covers')">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="albumArtTask?.state === 'running'"
          :aria-label="t('Complete missing album covers')"
          @click="requestAlbumBackfill"
        >
          <Download :size="16" aria-hidden="true" />
        </button>
      </div>

      <button
        class="btn btn-square btn-ghost btn-sm"
        type="button"
        :disabled="!visibleTracks.length || loading"
        :aria-label="selectionCount ? t('Play selected tracks') : t('Play Collection')"
        :title="selectionCount ? t('Play selected tracks') : t('Play Collection')"
        @click="playCollection"
      >
        <Play :size="16" aria-hidden="true" />
      </button>
    </div>

    <div
      v-if="albumArtTask && (albumArtTask.state === 'running' || albumArtTask.state === 'paused')"
      class="shrink-0 border-b border-base-300 bg-base-200 px-3 py-2"
      role="status"
    >
      <div class="flex items-center gap-3 text-xs">
        <LoaderCircle v-if="albumArtTask.state === 'running'" class="animate-spin" :size="14" aria-hidden="true" />
        <Download v-else :size="14" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">
          {{ albumArtTask.currentAlbum || (albumArtTask.state === 'paused' ? t('Album cover completion paused') : t('Completing album covers')) }}
        </span>
        <span class="shrink-0 tabular-nums">{{ albumArtTask.processed }} / {{ albumArtTask.total }}</span>
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="albumArtTask.state === 'running' ? t('Pause album cover completion') : t('Resume album cover completion')"
          @click="albumArtTask.state === 'running' ? pauseAlbumBackfill() : resumeAlbumBackfill()"
        >
          <Pause v-if="albumArtTask.state === 'running'" :size="16" aria-hidden="true" />
          <Play v-else :size="16" aria-hidden="true" />
        </button>
      </div>
      <progress class="progress mt-1.5 h-1" :value="albumTaskPercent" max="100"></progress>
    </div>

    <div
      v-if="metadataTask && (metadataTask.state === 'running' || metadataTask.state === 'paused')"
      class="shrink-0 border-b border-base-300 bg-base-200 px-3 py-2"
      role="status"
    >
      <div class="flex items-center gap-3 text-xs">
        <LoaderCircle v-if="metadataTask.state === 'running'" class="animate-spin" :size="14" aria-hidden="true" />
        <Tags v-else :size="14" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">
          {{ metadataTask.currentTrack || (metadataTask.state === 'paused' ? t('Metadata lookup paused') : t('Looking up metadata')) }}
        </span>
        <span class="shrink-0 tabular-nums">{{ metadataTask.processed }} / {{ metadataTask.total }}</span>
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="metadataTask.state === 'running' ? t('Pause metadata lookup') : t('Resume metadata lookup')"
          @click="metadataTask.state === 'running' ? pauseMetadataLookup() : resumeMetadataLookup()"
        >
          <Pause v-if="metadataTask.state === 'running'" :size="16" aria-hidden="true" />
          <Play v-else :size="16" aria-hidden="true" />
        </button>
      </div>
      <progress class="progress mt-1.5 h-1" :value="metadataTaskPercent" max="100"></progress>
    </div>

    <div v-if="loading && !detail" class="grid min-h-0 flex-1 place-items-center" role="status">
      <RefreshCw class="animate-spin text-muted" :size="24" aria-hidden="true" />
      <span class="sr-only">{{ t("Loading Collection") }}</span>
    </div>

    <div v-else-if="!detail?.items.length" class="grid min-h-0 flex-1 place-items-center p-8 text-center">
      <div class="max-w-sm">
        <ListMusic class="mx-auto text-base-content/35" :size="34" aria-hidden="true" />
        <h2 class="mt-3 text-sm font-semibold">{{ t("This Collection is empty") }}</h2>
      </div>
    </div>

    <div v-else-if="!visibleTracks.length" class="grid min-h-0 flex-1 place-items-center p-8 text-center">
      <div class="max-w-sm">
        <Search class="mx-auto text-base-content/35" :size="34" aria-hidden="true" />
        <h2 class="mt-3 text-sm font-semibold">{{ t("No matching tracks") }}</h2>
      </div>
    </div>

    <div
      v-else
      ref="scrollViewport"
      class="relative min-h-0 flex-1 overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-base-content/25"
      role="table"
      :aria-label="t('Collection tracks')"
      :aria-rowcount="visibleTracks.length + groups.length"
      :aria-colcount="visibleColumns.length"
      :aria-activedescendant="focusedItemId ? `collection-row-${focusedItemId}` : undefined"
      tabindex="0"
      @keydown="handleGridKeydown"
      @contextmenu="openColumnMenu"
    >
      <div
        class="sticky top-0 z-30 grid h-8 border-b border-base-300 bg-base-200 text-xs font-medium"
        :style="tableGridStyle"
        role="row"
        @contextmenu="openColumnMenu"
      >
        <div
          v-for="column in visibleColumns"
          :key="column.id"
          class="group relative flex min-w-0 items-center border-r border-base-300 last:border-r-0"
          :class="column.definition.numeric ? 'justify-end' : ''"
          role="columnheader"
          :aria-label="column.id === 'playing' ? t('Playback status') : t(column.definition.label)"
          :aria-sort="sortAria(column.definition.sortField)"
          :draggable="!resizing"
          @dragstart="beginColumnDrag($event, column.id)"
          @dragover.prevent
          @drop="dropColumn($event, column.id)"
          @dragend="finishColumnDrag"
        >
          <button
            class="flex h-full min-w-0 flex-1 items-center gap-1 truncate px-2 text-left"
            :class="[
              column.definition.numeric ? 'justify-end text-right' : '',
              column.definition.sortField ? 'cursor-pointer hover:bg-base-300' : 'cursor-grab',
            ]"
            type="button"
            :disabled="!column.definition.sortField"
            @click="clickSort(column.definition.sortField)"
          >
            <Volume2 v-if="column.id === 'playing'" :size="13" aria-hidden="true" />
            <span v-else class="truncate">{{ t(column.definition.label) }}</span>
            <ChevronUp
              v-if="sortField === column.definition.sortField && sortDirection === 'ascending'"
              class="shrink-0"
              :size="12"
              aria-hidden="true"
            />
            <ChevronDown
              v-else-if="sortField === column.definition.sortField && sortDirection === 'descending'"
              class="shrink-0"
              :size="12"
              aria-hidden="true"
            />
          </button>
          <button
            class="absolute -right-1 top-0 z-10 h-full w-2 cursor-col-resize opacity-0 group-hover:opacity-100"
            type="button"
            :aria-label="t('Resize {column} column', { column: t(column.definition.label || 'playback status') })"
            @pointerdown="beginResize($event, column)"
            @dblclick.stop="autoFitColumn(column.id)"
          >
            <span class="mx-auto block h-full w-px bg-base-content/35"></span>
          </button>
        </div>
      </div>

      <div role="rowgroup" :style="tableWidthStyle">
        <template v-for="group in groups" :key="group.id">
          <div
            class="flex cursor-default items-stretch border-b border-base-300 text-xs"
            :class="isGroupSelected(group) ? 'bg-neutral text-neutral-content' : 'bg-base-200 hover:bg-base-300'"
            :style="{ height: `${rowHeight * 2}px` }"
            role="row"
            data-album-row
            :aria-selected="isGroupSelected(group)"
            draggable="true"
            @click="selectGroup($event, group)"
            @dblclick="playGroup(group, true)"
            @contextmenu.stop="openGroupMenu($event, group)"
            @dragstart="beginGroupDrag($event, group)"
          >
            <button
              class="grid w-7 shrink-0 place-items-center"
              type="button"
              :aria-label="collapsedGroupIds.has(group.id) ? t('Expand album') : t('Collapse album')"
              @click.stop="toggleGroup(group)"
            >
              <ChevronRight
                class="transition-transform"
                :class="{ 'rotate-90': !collapsedGroupIds.has(group.id) }"
                :size="16"
                aria-hidden="true"
              />
            </button>
            <div
              class="m-1 grid shrink-0 place-items-center overflow-hidden rounded-sm bg-base-300"
              :style="{ width: `${albumCoverSize}px`, height: `${albumCoverSize}px` }"
              :title="coverResult(group)?.message || undefined"
            >
              <img
                v-if="coverUrl(group)"
                class="size-full object-cover"
                :src="coverUrl(group)!"
                :alt="t('{title} cover', { title: group.title || t('Ungrouped tracks') })"
              />
              <div
                v-else-if="group.localAlbumGroupId && !coverResult(group)"
                class="skeleton size-full rounded-none"
                :aria-label="t('Loading album cover')"
              ></div>
              <Disc3 v-else :size="Math.max(20, albumCoverSize * 0.48)" :stroke-width="1.25" aria-hidden="true" />
            </div>
            <div class="flex min-w-0 flex-1 flex-col justify-center px-3">
              <div class="flex min-w-0 items-center gap-2">
                <span class="truncate text-sm font-semibold" :title="group.title || t('Ungrouped tracks')">
                  {{ group.title || t("Ungrouped tracks") }}
                </span>
                <span
                  v-if="group.year"
                  class="shrink-0 tabular-nums"
                  :class="isGroupSelected(group) ? 'text-neutral-content' : 'text-muted'"
                >
                  {{ group.year }}
                </span>
                <span
                  v-if="coverResult(group)?.status === 'failed' && !coverResult(group)?.failedTracks"
                  class="shrink-0 text-error"
                  :title="coverResult(group)?.message || t('Album cover lookup failed')"
                >
                  {{ t("Cover failed") }}
                </span>
                <span
                  v-else-if="coverResult(group)?.failedTracks"
                  class="shrink-0 text-warning"
                  :title="coverResult(group)?.message || t('Some album files could not be updated')"
                >
                  {{ t("{count} failed", { count: coverResult(group)!.failedTracks }) }}
                </span>
                <button
                  v-if="coverResult(group)?.status === 'needsReview'"
                  class="btn btn-square btn-ghost btn-sm shrink-0"
                  type="button"
                  :aria-label="t('Review album cover matches')"
                  :title="t('Review cover matches')"
                  @click.stop="reviewGroupCover(group)"
                >
                  <Info :size="16" aria-hidden="true" />
                </button>
              </div>
              <div
                class="mt-0.5 flex min-w-0 items-center gap-2"
                :class="isGroupSelected(group) ? 'text-neutral-content' : 'text-muted'"
              >
                <span class="truncate">{{ group.albumArtist || (group.isUngrouped ? t('Missing album metadata') : t('Unknown artist')) }}</span>
                <span class="shrink-0">&middot;</span>
                <span class="shrink-0 tabular-nums">
                  {{ group.tracks.length === group.totalTracks
                    ? t("{count} tracks", { count: group.totalTracks })
                    : t("{matched} / {total} matched", { matched: group.tracks.length, total: group.totalTracks }) }}
                </span>
                <span class="hidden shrink-0 sm:inline">&middot; {{ formatCollectionLongDuration(group.totalDurationSeconds) }}</span>
              </div>
            </div>
          </div>

          <div
            v-for="track in collapsedGroupIds.has(group.id) ? [] : group.tracks"
            :id="`collection-row-${track.item.id}`"
            :key="track.item.id"
            class="grid cursor-default border-b border-b-base-300/60 border-l-2 border-l-transparent text-xs"
            :class="[
              track.item.position % 2 === 1 ? 'bg-base-200/35' : 'bg-base-100',
              selectedItemIds.has(track.item.id) ? 'bg-neutral text-neutral-content' : 'hover:bg-base-200',
              isActive(track)
                ? selectedItemIds.has(track.item.id)
                  ? 'border-l-primary ring-1 ring-inset ring-primary/40'
                  : 'border-l-primary bg-primary/10 hover:bg-primary/15'
                : '',
              focusedItemId === track.item.id ? 'outline outline-1 -outline-offset-1 outline-base-content/40' : '',
            ]"
            :style="{ ...tableGridStyle, height: `${rowHeight}px` }"
            role="row"
            data-track-row
            :data-collection-item-id="track.item.id"
            :aria-selected="selectedItemIds.has(track.item.id)"
            :aria-current="isActive(track) ? 'true' : undefined"
            draggable="true"
            @click="selectTrack($event, track)"
            @dblclick="playVisibleTrack(track)"
            @contextmenu.stop="openRowMenu($event, track)"
            @dragstart="beginTrackDrag($event, track)"
          >
            <div
              v-for="column in visibleColumns"
              :key="column.id"
              class="flex min-w-0 items-center truncate border-r border-base-300/50 px-2 last:border-r-0"
              :class="column.definition.numeric ? 'justify-end text-right tabular-nums' : ''"
              role="cell"
              :title="column.id === 'playing' ? undefined : displayValue(track, column.id)"
            >
              <template v-if="column.id === 'playing'">
                <Volume2
                  v-if="isActive(track) && isPlaying"
                  :class="selectedItemIds.has(track.item.id) ? 'text-neutral-content' : 'text-primary'"
                  :size="13"
                  :aria-label="t('Playing')"
                />
                <Pause
                  v-else-if="isActive(track)"
                  :class="selectedItemIds.has(track.item.id) ? 'text-neutral-content' : 'text-primary'"
                  :size="13"
                  :aria-label="t('Paused')"
                />
              </template>
              <template v-else-if="column.id === 'title'">
                <Radio
                  v-if="track.item.kind === 'online'"
                  class="mr-1.5 shrink-0 opacity-65"
                  :size="12"
                  :aria-label="t('Online track')"
                />
                <span class="truncate">{{ displayValue(track, column.id) }}</span>
              </template>
              <span v-else class="truncate">{{ displayValue(track, column.id) }}</span>
            </div>
          </div>
        </template>
      </div>
    </div>

    <div class="flex h-7 shrink-0 items-center gap-3 border-t border-base-300 bg-base-200 px-3 text-xs">
      <span class="min-w-0 flex-1 truncate">{{ queueStatus || resultSummary }}</span>
      <span v-if="selectionCount" class="shrink-0 tabular-nums">{{ t("{count} selected", { count: formatNumber(selectionCount) }) }}</span>
      <span v-if="searchInput.trim()" class="hidden shrink-0 text-muted xl:inline">{{ activeSortLabel }}</span>
      <span v-if="detail" class="hidden shrink-0 text-muted 2xl:inline">
        {{ t("{local} local · {online} online", { local: detail.collection.localCount, online: detail.collection.onlineCount }) }}
      </span>
    </div>
  </section>

  <ul
    v-if="rowMenu"
    class="menu fixed z-50 w-64 border border-base-300 bg-base-100 p-2 shadow-xl"
    :style="{ left: `${rowMenu.x}px`, top: `${rowMenu.y}px` }"
    data-menu-surface
    :aria-label="t('Collection track actions')"
  >
    <li><button type="button" @click="playSelection(true)"><Play :size="16" aria-hidden="true" />{{ t("Play selection") }}</button></li>
    <li><button type="button" @click="playSelection(false)"><ListMusic :size="16" aria-hidden="true" />{{ t("Set playback queue") }}</button></li>
    <li><button type="button" @click="requestCollectionAction(false)"><ListPlus :size="16" aria-hidden="true" />{{ t("Add selection to Collection") }}</button></li>
    <li><button type="button" @click="requestCollectionAction(true)"><FolderPlus :size="16" aria-hidden="true" />{{ t("New Collection from selection") }}</button></li>
    <li v-if="selectedLocalItemIds.length"><button type="button" :disabled="metadataTask?.state === 'running' || metadataTask?.state === 'paused'" @click="requestMetadataLookup"><Tags :size="16" aria-hidden="true" />{{ t("Look up metadata") }}</button></li>
    <li v-if="rowMenu.track.item.localTrack" class="my-1 h-px bg-base-300"></li>
    <li v-if="rowMenu.track.item.localTrack"><button type="button" @click="revealContextTrack"><FolderSearch :size="16" aria-hidden="true" />{{ t("Show in file manager") }}</button></li>
    <li v-if="rowMenu.track.item.localTrack"><button type="button" @click="copyContextPath"><Clipboard :size="16" aria-hidden="true" />{{ t("Copy file path") }}</button></li>
    <li v-if="rowMenu.track.item.localTrack"><button type="button" @click="showProperties"><Info :size="16" aria-hidden="true" />{{ t("Properties") }}</button></li>
    <li v-if="!isSmartCollection" class="my-1 h-px bg-base-300"></li>
    <li v-if="!isSmartCollection"><button class="text-error" type="button" :disabled="removing" @click="removeSelection"><Trash2 :size="16" aria-hidden="true" />{{ t("Remove selection from Collection") }}</button></li>
  </ul>

  <ul
    v-if="groupMenu"
    class="menu fixed z-50 w-64 border border-base-300 bg-base-100 p-2 shadow-xl"
    :style="{ left: `${groupMenu.x}px`, top: `${groupMenu.y}px` }"
    data-menu-surface
    :aria-label="t('Collection album actions')"
  >
    <li><button type="button" @click="playGroup(groupMenu.group, true)"><Play :size="16" aria-hidden="true" />{{ t("Play album group") }}</button></li>
    <li><button type="button" @click="playGroup(groupMenu.group, false)"><ListMusic :size="16" aria-hidden="true" />{{ t("Set group as queue") }}</button></li>
    <li><button type="button" @click="requestCollectionAction(false)"><ListPlus :size="16" aria-hidden="true" />{{ t("Add selection to Collection") }}</button></li>
    <li><button type="button" @click="requestCollectionAction(true)"><FolderPlus :size="16" aria-hidden="true" />{{ t("New Collection from selection") }}</button></li>
    <li v-if="selectedLocalItemIds.length"><button type="button" :disabled="metadataTask?.state === 'running' || metadataTask?.state === 'paused'" @click="requestMetadataLookup"><Tags :size="16" aria-hidden="true" />{{ t("Look up metadata") }}</button></li>
    <li v-if="coverResult(groupMenu.group)?.status === 'needsReview'"><button type="button" @click="reviewGroupCover(groupMenu.group)"><Info :size="16" aria-hidden="true" />{{ t("Review cover matches") }}</button></li>
    <li class="my-1 h-px bg-base-300"></li>
    <li><button type="button" @click="toggleGroup(groupMenu.group)"><ChevronRight :class="{ 'rotate-90': !collapsedGroupIds.has(groupMenu.group.id) }" :size="16" aria-hidden="true" />{{ collapsedGroupIds.has(groupMenu.group.id) ? t("Expand album") : t("Collapse album") }}</button></li>
    <li v-if="!isSmartCollection"><button class="text-error" type="button" :disabled="removing" @click="removeSelection"><Trash2 :size="16" aria-hidden="true" />{{ t("Remove album from Collection") }}</button></li>
  </ul>

  <div
    v-if="columnMenu"
    class="fixed z-50 max-h-[70vh] w-64 overflow-y-auto border border-base-300 bg-base-100 shadow-xl"
    :style="{ left: `${columnMenu.x}px`, top: `${columnMenu.y}px` }"
    data-menu-surface
    :aria-label="t('Collection columns')"
  >
    <div class="flex h-9 items-center border-b border-base-300 px-3 text-xs font-semibold">{{ t("Columns") }}</div>
    <ul class="menu menu-sm p-2">
      <li v-for="column in columns" :key="column.id">
        <button type="button" @click="toggleColumn(column.id)">
          <span class="grid size-4 place-items-center"><Check v-if="column.visible" :size="16" aria-hidden="true" /></span>
          {{ t(COLLECTION_COLUMN_DEFINITIONS.find((definition) => definition.id === column.id)?.label || "Playback status") }}
        </button>
      </li>
      <li class="my-1 h-px bg-base-300"></li>
      <li><button type="button" @click="resetColumns"><RotateCcw :size="16" aria-hidden="true" />{{ t("Reset columns") }}</button></li>
    </ul>
  </div>

  <div v-if="networkPermissionOpen" class="modal modal-open" role="dialog" aria-modal="true" :aria-label="t('Online metadata permission')">
    <div class="modal-box max-w-lg rounded">
      <h2 class="text-base font-semibold">{{ t("Enable online metadata completion") }}</h2>
      <p class="mt-3 text-sm leading-6 text-muted">
        {{ t("Fika Music will send local album and track metadata to MusicBrainz, download Front artwork from Cover Art Archive, and write verified matches into empty audio tags. Online Collection entries are not modified.") }}
      </p>
      <div class="modal-action">
        <button class="btn" type="button" @click="dismissOnlineMetadata">{{ t("Not now") }}</button>
        <button class="btn btn-neutral" type="button" @click="authorizeOnlineMetadata">{{ t("Enable") }}</button>
      </div>
    </div>
    <button class="modal-backdrop" type="button" :aria-label="t('Decline online metadata')" @click="dismissOnlineMetadata"></button>
  </div>

  <div v-if="coverReview" class="modal modal-open" role="dialog" aria-modal="true" :aria-label="t('Choose album cover match')">
    <div class="modal-box max-w-xl rounded">
      <div class="flex items-start gap-3">
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ coverReview.group.title }}</h2>
          <p class="mt-1 text-sm text-muted">{{ coverReview.message || t("Choose the matching MusicBrainz release group.") }}</p>
        </div>
        <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Close cover matches')" @click="coverReview = null"><X :size="16" aria-hidden="true" /></button>
      </div>
      <ul class="menu mt-4 max-h-80 overflow-y-auto border border-base-300 p-2">
        <li v-for="candidate in coverReview.candidates" :key="candidate.releaseGroupId">
          <button class="items-start" type="button" @click="chooseCoverCandidate(candidate)">
            <Disc3 class="mt-0.5 shrink-0" :size="18" aria-hidden="true" />
            <span class="min-w-0 text-left">
              <span class="block truncate font-medium">{{ candidate.title }}</span>
              <span class="block truncate text-xs text-muted">{{ candidate.artist }}{{ candidate.year ? ` - ${candidate.year}` : "" }} - score {{ candidate.score }}</span>
            </span>
          </button>
        </li>
      </ul>
    </div>
    <button class="modal-backdrop" type="button" :aria-label="t('Close cover matches')" @click="coverReview = null"></button>
  </div>

  <div v-if="metadataConfirmOpen" class="modal modal-open" role="dialog" aria-modal="true" :aria-label="t('Confirm metadata lookup')">
    <div class="modal-box max-w-md rounded">
      <h2 class="text-base font-semibold">{{ t("Look up metadata for {count} local tracks?", { count: formatNumber(selectedLocalItemIds.length) }) }}</h2>
      <p class="mt-3 text-sm leading-6 text-muted">
        {{ t("MusicBrainz highest-confidence matches will fill empty tags only. Existing tag values are preserved. Online Collection entries are skipped.") }}
      </p>
      <div class="modal-action">
        <button class="btn" type="button" @click="metadataConfirmOpen = false">{{ t("Cancel") }}</button>
        <button class="btn btn-neutral" type="button" @click="startMetadataLookup">{{ t("Start") }}</button>
      </div>
    </div>
    <button class="modal-backdrop" type="button" :aria-label="t('Cancel metadata lookup')" @click="metadataConfirmOpen = false"></button>
  </div>

  <div v-if="propertiesTrack" class="modal modal-open" role="dialog" aria-modal="true" :aria-label="t('Track properties')">
    <div class="modal-box max-w-2xl rounded">
      <div class="flex items-start gap-3">
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold" :title="propertiesTrack.title">{{ propertiesTrack.title }}</h2>
          <p class="mt-0.5 truncate text-xs text-muted" :title="propertiesTrack.filePath">{{ propertiesTrack.filePath }}</p>
        </div>
        <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Close properties')" :title="t('Close')" @click="propertiesTrack = null"><X :size="16" aria-hidden="true" /></button>
      </div>
      <dl class="mt-5 grid grid-cols-[8rem_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
        <template
          v-for="entry in [
            [t('Artist'), propertiesTrack.artist || t('Unknown artist')],
            [t('Album'), propertiesTrack.album || t('Unknown album')],
            [t('Album artist'), propertiesTrack.albumArtist || ''],
            [t('Genre'), propertiesTrack.genre || ''],
            [t('Year'), propertiesTrack.year?.toString() || ''],
            [t('Track / disc'), `${propertiesTrack.trackNumber ?? '-'} / ${propertiesTrack.discNumber ?? '-'}`],
            [t('Duration'), formatCollectionDuration(propertiesTrack.durationSeconds)],
            [t('Format'), [propertiesTrack.codec, propertiesTrack.bitrateKbps ? `${propertiesTrack.bitrateKbps} kbps` : ''].filter(Boolean).join(' / ')],
            [t('File size'), t('{count} bytes', { count: formatNumber(propertiesTrack.fileSizeBytes) })],
            [t('Play count'), formatNumber(propertiesTrack.playCount)],
          ]"
          :key="entry[0]"
        >
          <dt class="text-muted">{{ entry[0] }}</dt>
          <dd class="min-w-0 break-words">{{ entry[1] || "-" }}</dd>
        </template>
      </dl>
    </div>
    <button class="modal-backdrop" type="button" :aria-label="t('Close properties')" @click="propertiesTrack = null"></button>
  </div>
</template>
