<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useVirtualizer } from "@tanstack/vue-virtual";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Clipboard,
  Disc3,
  Download,
  FolderSearch,
  Info,
  ListFilter,
  ListMusic,
  LoaderCircle,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Search,
  SlidersHorizontal,
  Tags,
  Volume2,
  X,
} from "@lucide/vue";
import {
  LIBRARY_COLUMN_DEFAULTS,
  loadLibraryPreferences,
  saveLibraryPreferences,
  type LibraryColumnId,
  type LibraryColumnPreference,
} from "../lib/library-preferences";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  AlbumArtSettings,
  AlbumArtTaskStatus,
  AlbumCoverCandidate,
  AlbumCoverResult,
  LibraryAlbumGroup,
  LibraryGroupToggleResult,
  LibraryPlaybackQueue,
  LibraryQueryPage,
  LibraryQueryRequest,
  LibrarySelectionRange,
  LibrarySelectionRequest,
  LibrarySortField,
  LibraryTextField,
  LibraryViewItem,
  LibraryViewRange,
  LocalTrack,
  MetadataLookupTaskStatus,
  ScanStatus,
} from "../generated/bindings";
import type { LayoutDensity } from "../lib/ui-preferences";

type ColumnDefinition = {
  id: LibraryColumnId;
  label: string;
  sortField: LibrarySortField | null;
  numeric?: boolean;
};

type MenuPosition = { x: number; y: number };
type LibrarySummary = { libraryTotal: number; filteredTotal: number };
type RowMenu = MenuPosition & { track: LocalTrack; trackIndex: number };
type GroupMenu = MenuPosition & { group: LibraryAlbumGroup; virtualIndex: number };

const props = defineProps<{
  activeTrackId: number | null;
  isPlaying: boolean;
  density: LayoutDensity;
  scanStatus: ScanStatus;
  scanMessage: string | null;
  canIndex: boolean;
}>();

const emit = defineEmits<{
  playbackQueue: [queue: LibraryPlaybackQueue, autoplay: boolean];
  error: [message: string];
  summary: [summary: LibrarySummary];
  index: [];
}>();

const columnDefinitions: ReadonlyArray<ColumnDefinition> = [
  { id: "playing", label: "", sortField: null },
  { id: "title", label: "Title", sortField: "title" },
  { id: "artist", label: "Artist", sortField: "artist" },
  { id: "album", label: "Album", sortField: "album" },
  { id: "albumArtist", label: "Album artist", sortField: "albumArtist" },
  { id: "genre", label: "Genre", sortField: "genre" },
  { id: "year", label: "Year", sortField: "year", numeric: true },
  { id: "codec", label: "Codec", sortField: "codec" },
  { id: "bitrateKbps", label: "Bitrate", sortField: "bitrateKbps", numeric: true },
  { id: "sampleRateHz", label: "Sample rate", sortField: "sampleRateHz", numeric: true },
  { id: "durationSeconds", label: "Time", sortField: "durationSeconds", numeric: true },
  { id: "trackNumber", label: "#", sortField: "trackNumber", numeric: true },
  { id: "discNumber", label: "Disc", sortField: "discNumber", numeric: true },
  { id: "fileName", label: "File name", sortField: "fileName" },
  { id: "filePath", label: "Path", sortField: "filePath" },
  { id: "fileSizeBytes", label: "Size", sortField: "fileSizeBytes", numeric: true },
  { id: "modifiedAt", label: "Modified", sortField: "modifiedAt" },
  { id: "indexedAt", label: "Indexed", sortField: "indexedAt" },
  { id: "playCount", label: "Plays", sortField: "playCount", numeric: true },
];

const searchFieldOptions: ReadonlyArray<{ id: LibraryTextField; label: string }> = [
  { id: "title", label: "Title" },
  { id: "artist", label: "Artist" },
  { id: "album", label: "Album" },
  { id: "albumArtist", label: "Album artist" },
  { id: "genre", label: "Genre" },
  { id: "codec", label: "Codec" },
  { id: "fileName", label: "File name" },
  { id: "filePath", label: "File path" },
];

const VIEW_FETCH_ALIGNMENT = 100;
const VIEW_FETCH_LIMIT = 200;
const MAX_CACHED_ITEMS = VIEW_FETCH_LIMIT * 8;
const MAX_CACHED_ALBUM_COVERS = 96;

const savedPreferences = loadLibraryPreferences();
const columns = ref(savedPreferences.columns.map((column) => ({ ...column })));
const searchFields = ref([...savedPreferences.searchFields]);
const persistedSortField = ref(savedPreferences.sortField);
const persistedSortDirection = ref(savedPreferences.sortDirection);
const sortField = ref(savedPreferences.sortField);
const sortDirection = ref(savedPreferences.sortDirection);
const searchInput = ref("");
const searchViewport = ref<HTMLInputElement | null>(null);
const scrollViewport = ref<HTMLElement | null>(null);
const searchScopeMenu = ref<HTMLDetailsElement | null>(null);
const itemsByIndex = shallowRef(new Map<number, LibraryViewItem>());
const snapshotId = ref("");
const total = ref(0);
const virtualTotal = ref(0);
const groupTotal = ref(0);
const libraryTotal = ref(0);
const totalDurationSeconds = ref(0);
const needsReindex = ref(false);
const isQuerying = ref(false);
const pendingOffsets = new Set<number>();
const collapsedGroupIds = ref(new Set<string>());
const selectionRanges = ref<LibrarySelectionRange[]>([]);
const excludedRanges = ref<LibrarySelectionRange[]>([]);
const selectAll = ref(false);
const selectionAnchor = ref<number | null>(null);
const focusedVirtualIndex = ref<number | null>(null);
const rowMenu = ref<RowMenu | null>(null);
const groupMenu = ref<GroupMenu | null>(null);
const columnMenu = ref<MenuPosition | null>(null);
const propertiesTrack = ref<LocalTrack | null>(null);
const queueStatus = ref<string | null>(null);
const isCreatingQueue = ref(false);
const draggedColumn = ref<LibraryColumnId | null>(null);
const resizing = ref<{ id: LibraryColumnId; startX: number; startWidth: number } | null>(null);

const albumArtSettings = ref<AlbumArtSettings>({ networkEnabled: false });
const albumCovers = shallowRef(new Map<string, AlbumCoverResult>());
const albumArtTask = ref<AlbumArtTaskStatus | null>(null);
const metadataTask = ref<MetadataLookupTaskStatus | null>(null);
const networkPermissionOpen = ref(false);
const networkPermissionDismissed = ref(false);
const pendingNetworkAction = ref<"backfill" | "metadata" | null>(null);
const coverReview = ref<{
  group: LibraryAlbumGroup;
  candidates: AlbumCoverCandidate[];
  message: string | null;
} | null>(null);
const metadataConfirmOpen = ref(false);
const pendingCoverIds = new Set<string>();
const queuedCoverIds = new Set<string>();
const coverQueue: string[] = [];

let queryGeneration = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;
let queueStatusTimer: ReturnType<typeof setTimeout> | null = null;
let searchWasEmpty = true;
let temporaryRelevanceSort = false;
let isPumpingCovers = false;
let unlistenAlbumArt: (() => void) | null = null;
let unlistenMetadata: (() => void) | null = null;

const rowHeight = computed(() => (props.density === "compact" ? 32 : 34));
const albumCoverSize = computed(() => rowHeight.value * 2 - 8);
const visibleColumns = computed(() =>
  columns.value
    .filter((column) => column.visible)
    .map((column) => ({
      ...column,
      definition: columnDefinitions.find((definition) => definition.id === column.id)!,
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
const selectionCount = computed(() => {
  if (selectAll.value) {
    return Math.max(0, total.value - rangesLength(excludedRanges.value));
  }
  return rangesLength(selectionRanges.value);
});
const scanPercent = computed(() => {
  if (props.scanStatus.discoveredFiles <= 0) {
    return props.scanStatus.isRunning ? 1 : 0;
  }
  return Math.round((props.scanStatus.scannedFiles / props.scanStatus.discoveredFiles) * 100);
});
const resultSummary = computed(() => {
  const duration = formatLongDuration(totalDurationSeconds.value);
  const groups = `${groupTotal.value.toLocaleString()} album${groupTotal.value === 1 ? "" : "s"}`;
  if (searchInput.value.trim()) {
    return `${total.value.toLocaleString()} of ${libraryTotal.value.toLocaleString()} tracks in ${groups} · ${duration}`;
  }
  return `${total.value.toLocaleString()} tracks in ${groups} · ${duration}`;
});
const activeSortLabel = computed(() => {
  if (sortField.value === "relevance") {
    return "Relevance";
  }
  return columnDefinitions.find((column) => column.sortField === sortField.value)?.label ?? "Sort";
});
const albumTaskPercent = computed(() => taskPercent(albumArtTask.value));
const metadataTaskPercent = computed(() => taskPercent(metadataTask.value));

const rowVirtualizer = useVirtualizer(
  computed(() => ({
    count: virtualTotal.value,
    getScrollElement: () => scrollViewport.value,
    estimateSize: () => rowHeight.value,
    overscan: 12,
    getItemKey: (index: number) => `${snapshotId.value}:${index}`,
  })),
);
const virtualRows = computed(() => rowVirtualizer.value.getVirtualItems());
const renderedRows = computed(() =>
  virtualRows.value.map((virtual) => ({ virtual, item: itemAt(virtual.index) })),
);
const totalVirtualHeight = computed(() => rowVirtualizer.value.getTotalSize());

watch(
  () => virtualRows.value.map((row) => row.index).join(","),
  () => {
    void ensureVisibleItems();
    scheduleVisibleCovers();
  },
);

watch(rowHeight, async () => {
  await nextTick();
  rowVirtualizer.value.measure();
});

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
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
  searchTimer = setTimeout(() => void runQuery(), 120);
});

watch(
  () => props.scanStatus.isRunning,
  (running, wasRunning) => {
    if (wasRunning && !running) {
      void runQuery();
    }
  },
);

onMounted(() => {
  window.addEventListener("pointerdown", handleWindowPointerDown);
  window.addEventListener("keydown", handleWindowKeydown);
  void initializeOnlineFeatures();
  void runQuery();
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", handleWindowPointerDown);
  window.removeEventListener("keydown", handleWindowKeydown);
  stopResize();
  unlistenAlbumArt?.();
  unlistenMetadata?.();
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
  if (queueStatusTimer) {
    clearTimeout(queueStatusTimer);
  }
});

async function initializeOnlineFeatures() {
  try {
    const [settings, artStatus, metadataStatus] = await Promise.all([
      invoke<AlbumArtSettings>(TAURI_COMMANDS.getAlbumArtSettings),
      invoke<AlbumArtTaskStatus>(TAURI_COMMANDS.getAlbumArtTaskStatus),
      invoke<MetadataLookupTaskStatus>(TAURI_COMMANDS.getMetadataLookupTaskStatus),
    ]);
    albumArtSettings.value = settings;
    albumArtTask.value = artStatus;
    metadataTask.value = metadataStatus;
  } catch (error) {
    emit("error", normalizeError(error));
  }
  try {
    unlistenAlbumArt = await listen<AlbumArtTaskStatus>("library:album-art-progress", (event) => {
      const previous = albumArtTask.value?.state;
      albumArtTask.value = event.payload;
      if (event.payload.state === "completed" && previous !== "completed") {
        clearReplaceableCoverResults();
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
          void runQuery();
        }
      },
    );
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function runQuery() {
  const generation = ++queryGeneration;
  isQuerying.value = true;
  closeMenus();
  const request: LibraryQueryRequest = {
    search: searchInput.value.trim(),
    searchFields: [...searchFields.value],
    sortField: sortField.value,
    sortDirection: sortDirection.value,
    collapsedGroupIds: [...collapsedGroupIds.value],
  };

  try {
    const page = await invoke<LibraryQueryPage>(TAURI_COMMANDS.queryLocalLibrary, { request });
    if (generation !== queryGeneration) {
      return;
    }
    snapshotId.value = page.snapshotId;
    total.value = page.total;
    virtualTotal.value = page.virtualTotal;
    groupTotal.value = page.groupTotal;
    libraryTotal.value = page.libraryTotal;
    totalDurationSeconds.value = page.totalDurationSeconds;
    needsReindex.value = page.needsReindex;
    itemsByIndex.value = new Map(page.items.map((item) => [item.index, item]));
    pendingOffsets.clear();
    clearSelection();
    emit("summary", { libraryTotal: page.libraryTotal, filteredTotal: page.total });
    await nextTick();
    rowVirtualizer.value.scrollToIndex(0, { align: "start" });
    void ensureVisibleItems();
    scheduleVisibleCovers();
  } catch (error) {
    if (generation === queryGeneration) {
      emit("error", normalizeError(error));
    }
  } finally {
    if (generation === queryGeneration) {
      isQuerying.value = false;
    }
  }
}

async function ensureVisibleItems() {
  if (!snapshotId.value || !virtualRows.value.length) {
    return;
  }
  const missing = virtualRows.value.find((row) => !itemsByIndex.value.has(row.index));
  if (!missing) {
    return;
  }
  await loadViewRange(missing.index);
  await nextTick();
  if (virtualRows.value.some((row) => !itemsByIndex.value.has(row.index))) {
    void ensureVisibleItems();
  }
}

async function loadViewRange(index: number) {
  const offset = Math.floor(index / VIEW_FETCH_ALIGNMENT) * VIEW_FETCH_ALIGNMENT;
  if (!snapshotId.value || pendingOffsets.has(offset)) {
    return;
  }
  pendingOffsets.add(offset);
  const requestedSnapshot = snapshotId.value;
  try {
    const range = await invoke<LibraryViewRange>(TAURI_COMMANDS.localLibraryViewRange, {
      snapshotId: requestedSnapshot,
      offset,
      limit: VIEW_FETCH_LIMIT,
    });
    if (snapshotId.value !== requestedSnapshot) {
      return;
    }
    const nextItems = new Map(itemsByIndex.value);
    range.items.forEach((item) => nextItems.set(item.index, item));
    itemsByIndex.value = pruneItemCache(nextItems);
    scheduleVisibleCovers();
  } catch (error) {
    if (snapshotId.value === requestedSnapshot) {
      emit("error", normalizeError(error));
    }
  } finally {
    pendingOffsets.delete(offset);
  }
}

function pruneItemCache(items: Map<number, LibraryViewItem>) {
  if (items.size <= MAX_CACHED_ITEMS) {
    return items;
  }
  const visible = virtualRows.value;
  const centerIndex = visible.length ? visible[Math.floor(visible.length / 2)].index : 0;
  const nearest = [...items.entries()]
    .sort(([leftIndex], [rightIndex]) => {
      const distance = Math.abs(leftIndex - centerIndex) - Math.abs(rightIndex - centerIndex);
      return distance || leftIndex - rightIndex;
    })
    .slice(0, MAX_CACHED_ITEMS)
    .sort(([leftIndex], [rightIndex]) => leftIndex - rightIndex);
  return new Map(nearest);
}

function itemAt(index: number) {
  return itemsByIndex.value.get(index);
}

function clickSort(column: ColumnDefinition) {
  if (!column.sortField || draggedColumn.value || resizing.value) {
    return;
  }
  if (sortField.value === column.sortField) {
    sortDirection.value = sortDirection.value === "ascending" ? "descending" : "ascending";
  } else {
    sortField.value = column.sortField;
    sortDirection.value = "ascending";
  }
  persistedSortField.value = sortField.value;
  persistedSortDirection.value = sortDirection.value;
  temporaryRelevanceSort = false;
  persistPreferences();
  void runQuery();
}

function restoreRelevanceSort() {
  sortField.value = "relevance";
  sortDirection.value = "descending";
  temporaryRelevanceSort = true;
  void runQuery();
}

function toggleSearchField(field: LibraryTextField) {
  if (searchFields.value.includes(field)) {
    if (searchFields.value.length === 1) {
      return;
    }
    searchFields.value = searchFields.value.filter((candidate) => candidate !== field);
  } else {
    searchFields.value = [...searchFields.value, field];
  }
  persistPreferences();
  void runQuery();
}

function clearSearch() {
  searchInput.value = "";
  searchViewport.value?.focus();
}

function openColumnMenu(event: MouseEvent) {
  event.preventDefault();
  rowMenu.value = null;
  groupMenu.value = null;
  columnMenu.value = menuPosition(event.clientX, event.clientY, 260, 520);
}

function toggleColumn(columnId: LibraryColumnId) {
  const target = columns.value.find((column) => column.id === columnId);
  if (!target) {
    return;
  }
  const visibleDataColumns = columns.value.filter(
    (column) => column.visible && column.id !== "playing",
  ).length;
  if (target.visible && target.id !== "playing" && visibleDataColumns === 1) {
    return;
  }
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
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
  }
}

function dropColumn(event: DragEvent, targetId: LibraryColumnId) {
  event.preventDefault();
  const sourceId = draggedColumn.value;
  draggedColumn.value = null;
  if (!sourceId || sourceId === targetId) {
    return;
  }
  const next = [...columns.value];
  const sourceIndex = next.findIndex((column) => column.id === sourceId);
  const targetIndex = next.findIndex((column) => column.id === targetId);
  if (sourceIndex < 0 || targetIndex < 0) {
    return;
  }
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
  if (!state) {
    return;
  }
  const column = columns.value.find((candidate) => candidate.id === state.id);
  if (!column) {
    return;
  }
  column.width = Math.min(640, Math.max(36, state.startWidth + event.clientX - state.startX));
  columns.value = [...columns.value];
}

function finishResize() {
  if (resizing.value) {
    persistPreferences();
  }
  stopResize();
}

function stopResize() {
  resizing.value = null;
  window.removeEventListener("pointermove", resizeColumn);
  window.removeEventListener("pointerup", finishResize);
}

function autoFitColumn(columnId: LibraryColumnId) {
  const column = columns.value.find((candidate) => candidate.id === columnId);
  const definition = columnDefinitions.find((candidate) => candidate.id === columnId);
  if (!column || !definition) {
    return;
  }
  const values = [...itemsByIndex.value.values()]
    .filter((item): item is LibraryViewItem & { track: LocalTrack } => Boolean(item.track))
    .map((item) => displayValue(item.track, columnId));
  const widest = [definition.label, ...values].reduce(
    (maximum, value) => Math.max(maximum, visualTextWidth(value)),
    0,
  );
  column.width = Math.min(640, Math.max(columnId === "playing" ? 36 : 56, widest + 28));
  columns.value = [...columns.value];
  persistPreferences();
}

function selectTrack(event: MouseEvent, virtualIndex: number, trackIndex: number) {
  focusedVirtualIndex.value = virtualIndex;
  if (event.shiftKey && selectionAnchor.value !== null) {
    const range = normalizedRange(selectionAnchor.value, trackIndex);
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = event.metaKey || event.ctrlKey
      ? mergeRanges([...selectionRanges.value, range])
      : [range];
  } else if (event.metaKey || event.ctrlKey) {
    toggleSelectedRange({ start: trackIndex, end: trackIndex });
    selectionAnchor.value = trackIndex;
  } else {
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = [{ start: trackIndex, end: trackIndex }];
    selectionAnchor.value = trackIndex;
  }
  scrollViewport.value?.focus({ preventScroll: true });
}

function selectGroup(event: MouseEvent, virtualIndex: number, group: LibraryAlbumGroup) {
  focusedVirtualIndex.value = virtualIndex;
  const groupRange = { start: group.startIndex, end: group.endIndex };
  if (event.shiftKey && selectionAnchor.value !== null) {
    const groupEndpoint = group.startIndex < selectionAnchor.value
      ? group.startIndex
      : group.endIndex;
    const range = normalizedRange(selectionAnchor.value, groupEndpoint);
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = event.metaKey || event.ctrlKey
      ? mergeRanges([...selectionRanges.value, range])
      : [range];
  } else if (event.metaKey || event.ctrlKey) {
    toggleSelectedRange(groupRange);
    selectionAnchor.value = group.startIndex;
  } else {
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = [groupRange];
    selectionAnchor.value = group.startIndex;
  }
  scrollViewport.value?.focus({ preventScroll: true });
}

function toggleSelectedRange(range: LibrarySelectionRange) {
  if (selectAll.value) {
    excludedRanges.value = rangeFullyCovered(excludedRanges.value, range)
      ? subtractRange(excludedRanges.value, range)
      : mergeRanges([...excludedRanges.value, range]);
    return;
  }
  selectionRanges.value = rangeFullyCovered(selectionRanges.value, range)
    ? subtractRange(selectionRanges.value, range)
    : mergeRanges([...selectionRanges.value, range]);
}

function openRowMenu(event: MouseEvent, trackIndex: number, track: LocalTrack) {
  event.preventDefault();
  if (!isTrackSelected(trackIndex)) {
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = [{ start: trackIndex, end: trackIndex }];
    selectionAnchor.value = trackIndex;
  }
  columnMenu.value = null;
  groupMenu.value = null;
  rowMenu.value = { ...menuPosition(event.clientX, event.clientY, 240, 330), track, trackIndex };
}

function openGroupMenu(event: MouseEvent, virtualIndex: number, group: LibraryAlbumGroup) {
  event.preventDefault();
  if (!isGroupSelected(group)) {
    selectAll.value = false;
    excludedRanges.value = [];
    selectionRanges.value = [{ start: group.startIndex, end: group.endIndex }];
    selectionAnchor.value = group.startIndex;
  }
  focusedVirtualIndex.value = virtualIndex;
  rowMenu.value = null;
  columnMenu.value = null;
  groupMenu.value = {
    ...menuPosition(event.clientX, event.clientY, 250, 350),
    group,
    virtualIndex,
  };
}

function isTrackSelected(trackIndex: number) {
  return selectAll.value
    ? !rangeContains(excludedRanges.value, trackIndex)
    : rangeContains(selectionRanges.value, trackIndex);
}

function isGroupSelected(group: LibraryAlbumGroup) {
  const range = { start: group.startIndex, end: group.endIndex };
  return selectAll.value
    ? !rangesIntersect(excludedRanges.value, range)
    : rangeFullyCovered(selectionRanges.value, range);
}

async function createPlaybackQueue(
  startIndex: number,
  autoplay: boolean,
  useSelection = false,
  selectionOverride: LibrarySelectionRequest | null = null,
) {
  if (!snapshotId.value || isCreatingQueue.value) {
    return;
  }
  isCreatingQueue.value = true;
  try {
    const queue = await invoke<LibraryPlaybackQueue>(TAURI_COMMANDS.createLocalLibraryPlaybackQueue, {
      snapshotId: snapshotId.value,
      startIndex,
      selection: selectionOverride ?? (useSelection ? selectionRequest() : null),
    });
    emit("playbackQueue", queue, autoplay);
    showQueueStatus(`${queue.total.toLocaleString()} track${queue.total === 1 ? "" : "s"} queued`);
    closeMenus();
  } catch (error) {
    emit("error", normalizeError(error));
  } finally {
    isCreatingQueue.value = false;
  }
}

function playGroup(group: LibraryAlbumGroup, autoplay: boolean) {
  void createPlaybackQueue(group.startIndex, autoplay, false, groupSelection(group));
}

function selectionRequest(): LibrarySelectionRequest | null {
  if (selectionCount.value === 0) {
    return null;
  }
  return {
    selectAll: selectAll.value,
    ranges: selectionRanges.value.map((range) => ({ ...range })),
    excludedRanges: excludedRanges.value.map((range) => ({ ...range })),
  };
}

function groupSelection(group: LibraryAlbumGroup): LibrarySelectionRequest {
  return {
    selectAll: false,
    ranges: [{ start: group.startIndex, end: group.endIndex }],
    excludedRanges: [],
  };
}

async function toggleGroup(group: LibraryAlbumGroup) {
  if (!snapshotId.value) {
    return;
  }
  const collapsed = !collapsedGroupIds.value.has(group.id);
  try {
    const result = await invoke<LibraryGroupToggleResult>(
      TAURI_COMMANDS.setLocalLibraryGroupCollapsed,
      { snapshotId: snapshotId.value, groupId: group.id, collapsed },
    );
    const nextCollapsed = new Set(collapsedGroupIds.value);
    if (collapsed) {
      nextCollapsed.add(group.id);
    } else {
      nextCollapsed.delete(group.id);
    }
    collapsedGroupIds.value = nextCollapsed;
    virtualTotal.value = result.virtualTotal;
    itemsByIndex.value = new Map(result.items.map((item) => [item.index, item]));
    pendingOffsets.clear();
    await nextTick();
    rowVirtualizer.value.scrollToIndex(result.groupVirtualIndex, { align: "start" });
    scheduleVisibleCovers();
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function revealContextTrack() {
  const track = rowMenu.value?.track;
  if (!track) {
    return;
  }
  try {
    await revealItemInDir(track.filePath);
    rowMenu.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

async function copyContextPath() {
  const track = rowMenu.value?.track;
  if (!track) {
    return;
  }
  try {
    await navigator.clipboard.writeText(track.filePath);
    showQueueStatus("Path copied");
    rowMenu.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function showProperties() {
  propertiesTrack.value = rowMenu.value?.track ?? null;
  rowMenu.value = null;
}

async function handleGridKeydown(event: KeyboardEvent) {
  if (event.key.toLowerCase() === "a" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    selectAll.value = true;
    selectionRanges.value = [];
    excludedRanges.value = [];
    selectionAnchor.value = 0;
    focusedVirtualIndex.value = virtualTotal.value ? 0 : null;
    return;
  }
  if (!virtualTotal.value) {
    return;
  }
  if (event.key === "Enter" && focusedVirtualIndex.value !== null) {
    const item = itemAt(focusedVirtualIndex.value);
    if (item?.kind === "track" && item.trackIndex !== null) {
      event.preventDefault();
      void createPlaybackQueue(item.trackIndex, true);
    } else if (item?.kind === "albumHeader" && item.group) {
      event.preventDefault();
      playGroup(item.group, true);
    }
    return;
  }
  const delta = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
  let nextIndex: number | null = null;
  if (delta) {
    nextIndex = Math.min(
      virtualTotal.value - 1,
      Math.max(0, (focusedVirtualIndex.value ?? 0) + delta),
    );
  } else if (event.key === "Home") {
    nextIndex = 0;
  } else if (event.key === "End") {
    nextIndex = virtualTotal.value - 1;
  }
  if (nextIndex === null) {
    return;
  }
  event.preventDefault();
  await loadViewRange(nextIndex);
  let item = itemAt(nextIndex);
  if (item?.kind === "albumContinuation") {
    const skipDirection = delta || (event.key === "End" ? -1 : 1);
    nextIndex = Math.min(virtualTotal.value - 1, Math.max(0, nextIndex + skipDirection));
    await loadViewRange(nextIndex);
    item = itemAt(nextIndex);
  }
  focusedVirtualIndex.value = nextIndex;
  if (item?.kind === "track" && item.trackIndex !== null) {
    updateKeyboardSelection(event, item.trackIndex);
  } else if (item?.kind === "albumHeader" && item.group) {
    updateKeyboardSelection(event, item.group.startIndex, item.group.endIndex);
  }
  rowVirtualizer.value.scrollToIndex(nextIndex, { align: "auto" });
}

function updateKeyboardSelection(event: KeyboardEvent, start: number, end = start) {
  const target = event.shiftKey && selectionAnchor.value !== null
    ? normalizedRange(selectionAnchor.value, start < selectionAnchor.value ? start : end)
    : { start, end };
  selectAll.value = false;
  excludedRanges.value = [];
  selectionRanges.value = [target];
  if (!event.shiftKey) {
    selectionAnchor.value = start;
  }
}

function clearSelection() {
  selectionRanges.value = [];
  excludedRanges.value = [];
  selectAll.value = false;
  selectionAnchor.value = null;
  focusedVirtualIndex.value = null;
}

function closeMenus() {
  rowMenu.value = null;
  groupMenu.value = null;
  columnMenu.value = null;
  if (searchScopeMenu.value) {
    searchScopeMenu.value.open = false;
  }
}

function handleWindowPointerDown(event: PointerEvent) {
  const target = event.target;
  if (target instanceof Element && target.closest("[data-menu-surface]")) {
    return;
  }
  closeMenus();
}

function handleWindowKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeMenus();
    propertiesTrack.value = null;
    coverReview.value = null;
    metadataConfirmOpen.value = false;
  }
}

function persistPreferences() {
  saveLibraryPreferences({
    columns: columns.value.map((column) => ({ ...column })),
    searchFields: [...searchFields.value],
    sortField: persistedSortField.value,
    sortDirection: persistedSortDirection.value,
  });
}

function showQueueStatus(message: string) {
  queueStatus.value = message;
  if (queueStatusTimer) {
    clearTimeout(queueStatusTimer);
  }
  queueStatusTimer = setTimeout(() => {
    queueStatus.value = null;
  }, 2_500);
}

function updatePlayCount(trackId: number, playCount: number) {
  const next = new Map(itemsByIndex.value);
  for (const [index, item] of next) {
    if (item.track?.id === trackId) {
      next.set(index, { ...item, track: { ...item.track, playCount } });
    }
  }
  itemsByIndex.value = next;
}

async function startFirstTrack() {
  if (!total.value) {
    return;
  }
  clearSelection();
  selectionRanges.value = [{ start: 0, end: 0 }];
  selectionAnchor.value = 0;
  const firstTrack = [...itemsByIndex.value.values()].find((item) => item.trackIndex === 0);
  focusedVirtualIndex.value = firstTrack?.index ?? null;
  await createPlaybackQueue(0, true);
}

function scheduleVisibleCovers() {
  const visibleGroupIds = new Set(
    virtualRows.value
      .map((row) => itemAt(row.index))
      .filter((item): item is LibraryViewItem & { group: LibraryAlbumGroup } =>
        item?.kind === "albumHeader" && item.group !== null && !item.group.isUngrouped)
      .map((item) => item.group.id),
  );
  for (let index = coverQueue.length - 1; index >= 0; index -= 1) {
    if (!visibleGroupIds.has(coverQueue[index])) {
      queuedCoverIds.delete(coverQueue[index]);
      coverQueue.splice(index, 1);
    }
  }
  for (const row of virtualRows.value) {
    const item = itemAt(row.index);
    const group = item?.kind === "albumHeader" ? item.group : null;
    if (!group || group.isUngrouped || albumCovers.value.has(group.id)) {
      continue;
    }
    if (!pendingCoverIds.has(group.id) && !queuedCoverIds.has(group.id)) {
      queuedCoverIds.add(group.id);
      coverQueue.push(group.id);
    }
  }
  void pumpCoverQueue();
}

async function pumpCoverQueue() {
  if (isPumpingCovers) {
    return;
  }
  isPumpingCovers = true;
  try {
    while (coverQueue.length) {
      const groupId = coverQueue.shift()!;
      queuedCoverIds.delete(groupId);
      if (albumCovers.value.has(groupId)) {
        continue;
      }
      pendingCoverIds.add(groupId);
      try {
        const result = await invoke<AlbumCoverResult>(TAURI_COMMANDS.resolveLocalAlbumCover, {
          groupId,
          releaseGroupId: null,
        });
        setAlbumCover(result);
        if (result.status === "authorizationRequired") {
          if (!networkPermissionDismissed.value) {
            networkPermissionOpen.value = true;
          }
        }
        if (result.status === "pending") {
          window.setTimeout(() => {
            deleteAlbumCover(groupId);
            scheduleVisibleCovers();
          }, 500);
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
  next.delete(result.groupId);
  next.set(result.groupId, result);
  while (next.size > MAX_CACHED_ALBUM_COVERS) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) {
      break;
    }
    next.delete(oldest);
  }
  albumCovers.value = next;
}

function deleteAlbumCover(groupId: string) {
  const next = new Map(albumCovers.value);
  next.delete(groupId);
  albumCovers.value = next;
}

function clearReplaceableCoverResults() {
  albumCovers.value = new Map(
    [...albumCovers.value].filter(([, result]) => result.status === "embedded"),
  );
}

function coverFor(groupId: string) {
  return albumCovers.value.get(groupId);
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
  if (!selectionRequest()) {
    return;
  }
  if (!albumArtSettings.value.networkEnabled) {
    pendingNetworkAction.value = "metadata";
    networkPermissionOpen.value = true;
    return;
  }
  if (selectionCount.value > 1) {
    metadataConfirmOpen.value = true;
  } else {
    void startMetadataLookup();
  }
}

async function startMetadataLookup() {
  const selection = selectionRequest();
  if (!selection || !snapshotId.value) {
    return;
  }
  metadataConfirmOpen.value = false;
  try {
    metadataTask.value = await invoke<MetadataLookupTaskStatus>(
      TAURI_COMMANDS.startLocalMetadataLookup,
      { snapshotId: snapshotId.value, selection },
    );
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
    albumCovers.value = new Map(
      [...albumCovers.value].filter(([, result]) => result.status !== "authorizationRequired"),
    );
    scheduleVisibleCovers();
    if (action === "backfill") {
      await startAlbumBackfill();
    } else if (action === "metadata") {
      requestMetadataLookup();
    }
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function dismissOnlineMetadata() {
  networkPermissionOpen.value = false;
  networkPermissionDismissed.value = true;
  pendingNetworkAction.value = null;
}

function reviewGroupCover(group: LibraryAlbumGroup) {
  const result = coverFor(group.id);
  if (!result?.candidates.length) {
    return;
  }
  coverReview.value = { group, candidates: result.candidates, message: result.message };
  closeMenus();
}

async function chooseCoverCandidate(candidate: AlbumCoverCandidate) {
  const review = coverReview.value;
  if (!review) {
    return;
  }
  try {
    const result = await invoke<AlbumCoverResult>(TAURI_COMMANDS.resolveLocalAlbumCover, {
      groupId: review.group.id,
      releaseGroupId: candidate.releaseGroupId,
    });
    setAlbumCover(result);
    coverReview.value = null;
  } catch (error) {
    emit("error", normalizeError(error));
  }
}

function displayValue(track: LocalTrack, columnId: LibraryColumnId) {
  switch (columnId) {
    case "title": return track.title;
    case "artist": return track.artist || "Unknown artist";
    case "album": return track.album || "Unknown album";
    case "albumArtist": return track.albumArtist || "";
    case "genre": return track.genre || "";
    case "year": return track.year?.toString() ?? "";
    case "codec": return track.codec || "";
    case "bitrateKbps": return track.bitrateKbps ? `${track.bitrateKbps} kbps` : "";
    case "sampleRateHz": return track.sampleRateHz ? formatSampleRate(track.sampleRateHz) : "";
    case "durationSeconds": return formatDuration(track.durationSeconds);
    case "trackNumber": return track.trackNumber?.toString() ?? "";
    case "discNumber": return track.discNumber?.toString() ?? "";
    case "fileName": return track.fileName;
    case "filePath": return track.filePath;
    case "fileSizeBytes": return formatFileSize(track.fileSizeBytes);
    case "modifiedAt": return formatTimestamp(track.modifiedAt);
    case "indexedAt": return formatTimestamp(track.indexedAt);
    case "playCount": return track.playCount.toLocaleString();
    default: return "";
  }
}

function sortAria(column: ColumnDefinition) {
  if (!column.sortField || sortField.value !== column.sortField) {
    return "none" as const;
  }
  return sortDirection.value;
}

function formatDuration(seconds: number | null) {
  if (!seconds) return "--:--";
  const minutes = Math.floor(seconds / 60);
  const remaining = Math.floor(seconds % 60);
  return `${minutes}:${remaining.toString().padStart(2, "0")}`;
}

function formatLongDuration(seconds: number) {
  if (seconds <= 0) return "0 min";
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.round((seconds % 3_600) / 60);
  return hours ? `${hours} hr ${minutes} min` : `${minutes} min`;
}

function formatFileSize(bytes: number) {
  return bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatSampleRate(hz: number) {
  return hz >= 1_000 ? `${(hz / 1_000).toFixed(hz % 1_000 ? 1 : 0)} kHz` : `${hz} Hz`;
}

function formatTimestamp(timestamp: number | null) {
  if (!timestamp) return "";
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  }).format(new Date(timestamp * 1_000));
}

function visualTextWidth(value: string) {
  return [...value].reduce(
    (width, character) => width + (character.codePointAt(0)! > 0xff ? 14 : 7),
    0,
  );
}

function menuPosition(x: number, y: number, width: number, height: number): MenuPosition {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  };
}

function normalizedRange(first: number, second: number): LibrarySelectionRange {
  return { start: Math.min(first, second), end: Math.max(first, second) };
}

function rangeContains(ranges: LibrarySelectionRange[], index: number) {
  return ranges.some((range) => index >= range.start && index <= range.end);
}

function rangeFullyCovered(ranges: LibrarySelectionRange[], target: LibrarySelectionRange) {
  return ranges.some((range) => range.start <= target.start && range.end >= target.end);
}

function rangesIntersect(ranges: LibrarySelectionRange[], target: LibrarySelectionRange) {
  return ranges.some((range) => range.start <= target.end && range.end >= target.start);
}

function rangesLength(ranges: LibrarySelectionRange[]) {
  return mergeRanges(ranges).reduce((sum, range) => sum + range.end - range.start + 1, 0);
}

function mergeRanges(ranges: LibrarySelectionRange[]) {
  const sorted = ranges
    .map((range) => normalizedRange(range.start, range.end))
    .sort((left, right) => left.start - right.start);
  const merged: LibrarySelectionRange[] = [];
  for (const range of sorted) {
    const previous = merged[merged.length - 1];
    if (previous && range.start <= previous.end + 1) {
      previous.end = Math.max(previous.end, range.end);
    } else {
      merged.push({ ...range });
    }
  }
  return merged;
}

function subtractRange(ranges: LibrarySelectionRange[], target: LibrarySelectionRange) {
  return ranges.flatMap((range) => {
    if (target.end < range.start || target.start > range.end) {
      return [range];
    }
    const result: LibrarySelectionRange[] = [];
    if (target.start > range.start) {
      result.push({ start: range.start, end: target.start - 1 });
    }
    if (target.end < range.end) {
      result.push({ start: target.end + 1, end: range.end });
    }
    return result;
  });
}

function taskPercent(task: { total: number; processed: number } | null) {
  if (!task?.total) return 0;
  return Math.round((task.processed / task.total) * 100);
}

function normalizeError(error: unknown) {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unexpected library error.";
}

defineExpose({ refresh: runQuery, startFirstTrack, updatePlayCount });
</script>

<template>
  <section
    class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded border border-base-300 bg-base-100"
    aria-label="Music library"
  >
    <div class="flex shrink-0 items-center gap-2 border-b border-base-300 p-2">
      <label class="input input-sm min-w-0 flex-1" aria-label="Search local library">
        <Search :size="15" aria-hidden="true" />
        <input
          ref="searchViewport"
          v-model="searchInput"
          class="min-w-0"
          type="text"
          role="searchbox"
          autocomplete="off"
          spellcheck="false"
          placeholder="Search title, artist, album"
        />
        <button
          v-if="searchInput"
          class="btn btn-square btn-ghost btn-xs"
          type="button"
          aria-label="Clear library search"
          title="Clear search"
          @click="clearSearch"
        >
          <X :size="13" aria-hidden="true" />
        </button>
      </label>

      <details ref="searchScopeMenu" class="dropdown dropdown-end" data-menu-surface>
        <summary class="btn btn-sm" title="Search fields">
          <SlidersHorizontal :size="15" aria-hidden="true" />
          <span class="hidden xl:inline">{{ searchFields.length }} fields</span>
        </summary>
        <ul class="menu dropdown-content z-50 mt-1 w-56 border border-base-300 bg-base-100 p-2 shadow-lg">
          <li class="menu-title">Search fields</li>
          <li v-for="field in searchFieldOptions" :key="field.id">
            <label class="flex cursor-pointer items-center gap-3">
              <input
                class="checkbox checkbox-sm"
                type="checkbox"
                :checked="searchFields.includes(field.id)"
                :disabled="searchFields.includes(field.id) && searchFields.length === 1"
                @change="toggleSearchField(field.id)"
              />
              <span>{{ field.label }}</span>
            </label>
          </li>
        </ul>
      </details>

      <button
        v-if="searchInput.trim() && sortField !== 'relevance'"
        class="btn btn-sm"
        type="button"
        title="Sort by relevance"
        @click="restoreRelevanceSort"
      >
        <ListFilter :size="15" aria-hidden="true" />
        <span class="hidden 2xl:inline">Relevance</span>
      </button>

      <div class="tooltip tooltip-left" data-tip="Complete missing album covers">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="albumArtTask?.state === 'running'"
          aria-label="Complete missing album covers"
          @click="requestAlbumBackfill"
        >
          <Download :size="16" aria-hidden="true" />
        </button>
      </div>

      <div class="tooltip tooltip-left" data-tip="Refresh library">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="isQuerying"
          aria-label="Refresh library"
          @click="runQuery"
        >
          <RefreshCw :class="{ 'animate-spin': isQuerying }" :size="16" aria-hidden="true" />
        </button>
      </div>
    </div>

    <div
      v-if="scanStatus.isRunning"
      class="shrink-0 border-b border-base-300 bg-base-200 px-3 py-2"
      role="status"
    >
      <div class="flex items-center gap-3 text-xs">
        <RefreshCw class="shrink-0 animate-spin" :size="14" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{{ scanMessage || "Indexing local tracks" }}</span>
        <span class="shrink-0 tabular-nums">
          {{ scanStatus.scannedFiles.toLocaleString() }} / {{ scanStatus.discoveredFiles.toLocaleString() }}
        </span>
        <span v-if="scanStatus.errorCount" class="shrink-0 text-warning">
          {{ scanStatus.errorCount }} errors
        </span>
      </div>
      <progress class="progress mt-1.5 h-1" :value="scanPercent" max="100"></progress>
    </div>

    <div
      v-else-if="needsReindex"
      class="flex shrink-0 items-center gap-3 border-b border-base-300 bg-base-200 px-3 py-2 text-xs"
      role="status"
    >
      <Info :size="14" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">Re-index to add year, genre and audio properties.</span>
      <button class="btn btn-xs" type="button" :disabled="!canIndex" @click="emit('index')">
        <RefreshCw :size="13" aria-hidden="true" />
        Re-index
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
          {{ albumArtTask.currentAlbum || (albumArtTask.state === 'paused' ? 'Album cover completion paused' : 'Completing album covers') }}
        </span>
        <span class="shrink-0 tabular-nums">{{ albumArtTask.processed }} / {{ albumArtTask.total }}</span>
        <button
          class="btn btn-square btn-ghost btn-xs"
          type="button"
          :aria-label="albumArtTask.state === 'running' ? 'Pause album cover completion' : 'Resume album cover completion'"
          @click="albumArtTask.state === 'running' ? pauseAlbumBackfill() : resumeAlbumBackfill()"
        >
          <Pause v-if="albumArtTask.state === 'running'" :size="13" aria-hidden="true" />
          <Play v-else :size="13" aria-hidden="true" />
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
          {{ metadataTask.currentTrack || (metadataTask.state === 'paused' ? 'Metadata lookup paused' : 'Looking up metadata') }}
        </span>
        <span class="shrink-0 tabular-nums">{{ metadataTask.processed }} / {{ metadataTask.total }}</span>
        <button
          class="btn btn-square btn-ghost btn-xs"
          type="button"
          :aria-label="metadataTask.state === 'running' ? 'Pause metadata lookup' : 'Resume metadata lookup'"
          @click="metadataTask.state === 'running' ? pauseMetadataLookup() : resumeMetadataLookup()"
        >
          <Pause v-if="metadataTask.state === 'running'" :size="13" aria-hidden="true" />
          <Play v-else :size="13" aria-hidden="true" />
        </button>
      </div>
      <progress class="progress mt-1.5 h-1" :value="metadataTaskPercent" max="100"></progress>
    </div>

    <div v-if="total === 0 && !isQuerying" class="grid min-h-0 flex-1 place-items-center p-8 text-center">
      <div class="max-w-sm">
        <Search v-if="searchInput.trim()" class="mx-auto text-base-content/35" :size="34" aria-hidden="true" />
        <ListMusic v-else class="mx-auto text-base-content/35" :size="34" aria-hidden="true" />
        <h2 class="mt-3 text-sm font-semibold">
          {{ searchInput.trim() ? "No matching tracks" : "No local tracks indexed" }}
        </h2>
      </div>
    </div>

    <div
      v-else
      ref="scrollViewport"
      class="relative min-h-0 flex-1 overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-base-content/25"
      role="table"
      aria-label="Library tracks"
      :aria-rowcount="virtualTotal"
      :aria-colcount="visibleColumns.length"
      :aria-activedescendant="focusedVirtualIndex !== null ? `library-row-${focusedVirtualIndex}` : undefined"
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
          :aria-label="column.id === 'playing' ? 'Playback status' : column.definition.label"
          :aria-sort="sortAria(column.definition)"
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
            @click="clickSort(column.definition)"
          >
            <Volume2 v-if="column.id === 'playing'" :size="13" aria-hidden="true" />
            <span v-else class="truncate">{{ column.definition.label }}</span>
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
            :aria-label="`Resize ${column.definition.label || 'playback status'} column`"
            @pointerdown="beginResize($event, column)"
            @dblclick.stop="autoFitColumn(column.id)"
          >
            <span class="mx-auto block h-full w-px bg-base-content/35"></span>
          </button>
        </div>
      </div>

      <div
        class="relative"
        :style="{ height: `${totalVirtualHeight}px`, width: `max(100%, ${minimumTableWidth}px)` }"
        role="rowgroup"
      >
        <template v-for="row in renderedRows" :key="String(row.virtual.key)">
          <div
            v-if="row.item?.kind === 'albumHeader' && row.item.group"
            :id="`library-row-${row.virtual.index}`"
            class="absolute left-0 top-0 z-10 flex cursor-default items-stretch border-b border-base-300 text-xs"
            :class="[
              isGroupSelected(row.item.group) ? 'bg-neutral text-neutral-content' : 'bg-base-200 hover:bg-base-300',
              focusedVirtualIndex === row.virtual.index ? 'outline outline-1 -outline-offset-1 outline-base-content/40' : '',
            ]"
            :style="{
              ...tableWidthStyle,
              height: `${rowHeight * 2}px`,
              transform: `translateY(${row.virtual.start}px)`,
            }"
            role="row"
            :aria-rowindex="row.virtual.index + 1"
            :aria-selected="isGroupSelected(row.item.group)"
            @click="selectGroup($event, row.virtual.index, row.item.group)"
            @dblclick="playGroup(row.item.group, true)"
            @contextmenu.stop="openGroupMenu($event, row.virtual.index, row.item.group)"
          >
            <button
              class="grid w-7 shrink-0 place-items-center"
              type="button"
              :aria-label="collapsedGroupIds.has(row.item.group.id) ? 'Expand album' : 'Collapse album'"
              @click.stop="toggleGroup(row.item.group)"
            >
              <ChevronRight
                class="transition-transform"
                :class="{ 'rotate-90': !collapsedGroupIds.has(row.item.group.id) }"
                :size="14"
                aria-hidden="true"
              />
            </button>
            <div
              class="m-1 grid shrink-0 place-items-center overflow-hidden rounded-sm bg-base-300"
              :style="{ width: `${albumCoverSize}px`, height: `${albumCoverSize}px` }"
              :title="coverFor(row.item.group.id)?.message || undefined"
            >
              <img
                v-if="coverFor(row.item.group.id)?.dataUrl"
                class="size-full object-cover"
                :src="coverFor(row.item.group.id)!.dataUrl!"
                :alt="`${row.item.group.title || 'Ungrouped tracks'} cover`"
              />
              <div
                v-else-if="!row.item.group.isUngrouped && !coverFor(row.item.group.id)"
                class="skeleton size-full rounded-none"
                aria-label="Loading album cover"
              ></div>
              <Disc3 v-else :size="Math.max(20, albumCoverSize * 0.48)" :stroke-width="1.25" aria-hidden="true" />
            </div>
            <div class="flex min-w-0 flex-1 flex-col justify-center px-3">
              <div class="flex min-w-0 items-center gap-2">
                <span class="truncate text-sm font-semibold" :title="row.item.group.title || 'Ungrouped tracks'">
                  {{ row.item.group.title || "Ungrouped tracks" }}
                </span>
                <span v-if="row.item.group.year" class="shrink-0 tabular-nums text-base-content/55">
                  {{ row.item.group.year }}
                </span>
                <span
                  v-if="coverFor(row.item.group.id)?.status === 'failed' && !coverFor(row.item.group.id)?.failedTracks"
                  class="shrink-0 text-error"
                  :title="coverFor(row.item.group.id)?.message || 'Album cover lookup failed'"
                >
                  Cover failed
                </span>
                <span
                  v-else-if="coverFor(row.item.group.id)?.failedTracks"
                  class="shrink-0 text-warning"
                  :title="coverFor(row.item.group.id)?.message || 'Some album files could not be updated'"
                >
                  {{ coverFor(row.item.group.id)!.failedTracks }} failed
                </span>
                <button
                  v-if="coverFor(row.item.group.id)?.status === 'needsReview'"
                  class="btn btn-square btn-ghost btn-xs shrink-0"
                  type="button"
                  aria-label="Review album cover matches"
                  title="Review cover matches"
                  @click.stop="reviewGroupCover(row.item.group)"
                >
                  <Info :size="13" aria-hidden="true" />
                </button>
              </div>
              <div class="mt-0.5 flex min-w-0 items-center gap-2 text-base-content/60">
                <span class="truncate">{{ row.item.group.albumArtist || (row.item.group.isUngrouped ? 'Missing album metadata' : 'Unknown artist') }}</span>
                <span class="shrink-0">·</span>
                <span class="shrink-0 tabular-nums">
                  {{ row.item.group.matchedTracks === row.item.group.totalTracks
                    ? `${row.item.group.totalTracks} tracks`
                    : `${row.item.group.matchedTracks} / ${row.item.group.totalTracks} matched` }}
                </span>
                <span class="hidden shrink-0 sm:inline">· {{ formatLongDuration(row.item.group.totalDurationSeconds) }}</span>
              </div>
            </div>
          </div>

          <div
            v-else-if="row.item?.kind === 'albumContinuation'"
            class="pointer-events-none absolute left-0 top-0"
            :style="{
              ...tableWidthStyle,
              height: `${rowHeight}px`,
              transform: `translateY(${row.virtual.start}px)`,
            }"
            aria-hidden="true"
          ></div>

          <div
            v-else-if="row.item?.kind === 'track' && row.item.track && row.item.trackIndex !== null"
            :id="`library-row-${row.virtual.index}`"
            class="absolute left-0 top-0 grid cursor-default border-b border-base-300/60 text-xs"
            :class="[
              row.item.trackIndex % 2 === 1 ? 'bg-base-200/35' : 'bg-base-100',
              isTrackSelected(row.item.trackIndex)
                ? 'bg-neutral text-neutral-content'
                : 'hover:bg-base-200',
              activeTrackId === row.item.track.id && !isTrackSelected(row.item.trackIndex) ? 'bg-base-300' : '',
              focusedVirtualIndex === row.virtual.index ? 'outline outline-1 -outline-offset-1 outline-base-content/40' : '',
            ]"
            :style="{
              ...tableGridStyle,
              height: `${rowHeight}px`,
              transform: `translateY(${row.virtual.start}px)`,
            }"
            role="row"
            :aria-rowindex="row.virtual.index + 1"
            :aria-selected="isTrackSelected(row.item.trackIndex)"
            @click="selectTrack($event, row.virtual.index, row.item.trackIndex)"
            @dblclick="createPlaybackQueue(row.item.trackIndex, true)"
            @contextmenu.stop="openRowMenu($event, row.item.trackIndex, row.item.track)"
          >
            <div
              v-for="column in visibleColumns"
              :key="column.id"
              class="flex min-w-0 items-center truncate border-r border-base-300/50 px-2 last:border-r-0"
              :class="column.definition.numeric ? 'justify-end text-right tabular-nums' : ''"
              role="cell"
              :title="column.id === 'playing' ? undefined : displayValue(row.item.track, column.id)"
            >
              <template v-if="column.id === 'playing'">
                <Volume2 v-if="activeTrackId === row.item.track.id && isPlaying" :size="13" aria-label="Playing" />
                <Pause v-else-if="activeTrackId === row.item.track.id" :size="13" aria-label="Paused" />
              </template>
              <span v-else class="truncate">{{ displayValue(row.item.track, column.id) }}</span>
            </div>
          </div>

          <div
            v-else
            class="absolute left-0 top-0 flex items-center border-b border-base-300/60 bg-base-100 px-3 text-xs text-base-content/35"
            :style="{
              ...tableWidthStyle,
              height: `${rowHeight}px`,
              transform: `translateY(${row.virtual.start}px)`,
            }"
            role="row"
          >
            Loading
          </div>
        </template>
      </div>
    </div>

    <div class="flex h-7 shrink-0 items-center gap-3 border-t border-base-300 bg-base-200 px-3 text-xs">
      <span class="min-w-0 flex-1 truncate">{{ queueStatus || resultSummary }}</span>
      <span v-if="selectionCount" class="shrink-0 tabular-nums">{{ selectionCount.toLocaleString() }} selected</span>
      <span v-if="searchInput.trim()" class="hidden shrink-0 text-base-content/60 xl:inline">{{ activeSortLabel }}</span>
    </div>
  </section>

  <ul
    v-if="rowMenu"
    class="menu fixed z-50 w-60 border border-base-300 bg-base-100 p-2 shadow-xl"
    :style="{ left: `${rowMenu.x}px`, top: `${rowMenu.y}px` }"
    data-menu-surface
    aria-label="Track actions"
  >
    <li><button type="button" :disabled="isCreatingQueue" @click="createPlaybackQueue(rowMenu.trackIndex, true, true)"><Play :size="15" aria-hidden="true" />Play selection</button></li>
    <li><button type="button" :disabled="isCreatingQueue" @click="createPlaybackQueue(rowMenu.trackIndex, false, true)"><ListMusic :size="15" aria-hidden="true" />Set playback queue</button></li>
    <li><button type="button" :disabled="metadataTask?.state === 'running' || metadataTask?.state === 'paused'" @click="requestMetadataLookup"><Tags :size="15" aria-hidden="true" />Look up metadata</button></li>
    <li class="my-1 h-px bg-base-300"></li>
    <li><button type="button" @click="revealContextTrack"><FolderSearch :size="15" aria-hidden="true" />Show in file manager</button></li>
    <li><button type="button" @click="copyContextPath"><Clipboard :size="15" aria-hidden="true" />Copy file path</button></li>
    <li><button type="button" @click="showProperties"><Info :size="15" aria-hidden="true" />Properties</button></li>
  </ul>

  <ul
    v-if="groupMenu"
    class="menu fixed z-50 w-64 border border-base-300 bg-base-100 p-2 shadow-xl"
    :style="{ left: `${groupMenu.x}px`, top: `${groupMenu.y}px` }"
    data-menu-surface
    aria-label="Album actions"
  >
    <li><button type="button" :disabled="isCreatingQueue" @click="playGroup(groupMenu.group, true)"><Play :size="15" aria-hidden="true" />Play album group</button></li>
    <li><button type="button" :disabled="isCreatingQueue" @click="playGroup(groupMenu.group, false)"><ListMusic :size="15" aria-hidden="true" />Set group as queue</button></li>
    <li><button type="button" :disabled="metadataTask?.state === 'running' || metadataTask?.state === 'paused'" @click="requestMetadataLookup"><Tags :size="15" aria-hidden="true" />Look up metadata</button></li>
    <li v-if="coverFor(groupMenu.group.id)?.status === 'needsReview'"><button type="button" @click="reviewGroupCover(groupMenu.group)"><Info :size="15" aria-hidden="true" />Review cover matches</button></li>
    <li class="my-1 h-px bg-base-300"></li>
    <li><button type="button" @click="toggleGroup(groupMenu.group)"><ChevronRight :class="{ 'rotate-90': !collapsedGroupIds.has(groupMenu.group.id) }" :size="15" aria-hidden="true" />{{ collapsedGroupIds.has(groupMenu.group.id) ? "Expand album" : "Collapse album" }}</button></li>
  </ul>

  <div
    v-if="columnMenu"
    class="fixed z-50 max-h-[70vh] w-64 overflow-y-auto border border-base-300 bg-base-100 shadow-xl"
    :style="{ left: `${columnMenu.x}px`, top: `${columnMenu.y}px` }"
    data-menu-surface
    aria-label="Library columns"
  >
    <div class="flex h-9 items-center border-b border-base-300 px-3 text-xs font-semibold">Columns</div>
    <ul class="menu menu-sm p-2">
      <li v-for="column in columns" :key="column.id">
        <button type="button" @click="toggleColumn(column.id)">
          <span class="grid size-4 place-items-center"><Check v-if="column.visible" :size="14" aria-hidden="true" /></span>
          {{ columnDefinitions.find((definition) => definition.id === column.id)?.label || "Playback status" }}
        </button>
      </li>
      <li class="my-1 h-px bg-base-300"></li>
      <li><button type="button" @click="resetColumns"><RotateCcw :size="14" aria-hidden="true" />Reset columns</button></li>
    </ul>
  </div>

  <div v-if="networkPermissionOpen" class="modal modal-open" role="dialog" aria-modal="true" aria-label="Online metadata permission">
    <div class="modal-box max-w-lg rounded">
      <h2 class="text-base font-semibold">Enable online metadata completion</h2>
      <p class="mt-3 text-sm leading-6 text-base-content/70">
        Fika Music will send album and track metadata to MusicBrainz, download Front artwork from Cover Art Archive, and write verified matches into empty audio tags. Album covers are embedded in every song in the matched album. Requests are rate-limited and ambiguous album matches require review.
      </p>
      <p class="mt-3 text-xs leading-5 text-base-content/55">
        Cover Art Archive images remain copyrighted by their respective owners. This permission is stored for this application and can be declined now.
      </p>
      <div class="modal-action">
        <button class="btn" type="button" @click="dismissOnlineMetadata">Not now</button>
        <button class="btn btn-neutral" type="button" @click="authorizeOnlineMetadata">Enable</button>
      </div>
    </div>
    <button class="modal-backdrop" type="button" aria-label="Decline online metadata" @click="dismissOnlineMetadata"></button>
  </div>

  <div v-if="coverReview" class="modal modal-open" role="dialog" aria-modal="true" aria-label="Choose album cover match">
    <div class="modal-box max-w-xl rounded">
      <div class="flex items-start gap-3">
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ coverReview.group.title }}</h2>
          <p class="mt-1 text-sm text-base-content/60">
            {{ coverReview.message || "Choose the matching MusicBrainz release group." }}
          </p>
        </div>
        <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Close cover matches" @click="coverReview = null"><X :size="16" aria-hidden="true" /></button>
      </div>
      <ul class="menu mt-4 max-h-80 overflow-y-auto border border-base-300 p-2">
        <li v-for="candidate in coverReview.candidates" :key="candidate.releaseGroupId">
          <button class="items-start" type="button" @click="chooseCoverCandidate(candidate)">
            <Disc3 class="mt-0.5 shrink-0" :size="18" aria-hidden="true" />
            <span class="min-w-0 text-left">
              <span class="block truncate font-medium">{{ candidate.title }}</span>
              <span class="block truncate text-xs text-base-content/60">{{ candidate.artist }}{{ candidate.year ? ` · ${candidate.year}` : "" }} · score {{ candidate.score }}</span>
            </span>
          </button>
        </li>
      </ul>
    </div>
    <button class="modal-backdrop" type="button" aria-label="Close cover matches" @click="coverReview = null"></button>
  </div>

  <div v-if="metadataConfirmOpen" class="modal modal-open" role="dialog" aria-modal="true" aria-label="Confirm metadata lookup">
    <div class="modal-box max-w-md rounded">
      <h2 class="text-base font-semibold">Look up metadata for {{ selectionCount.toLocaleString() }} tracks?</h2>
      <p class="mt-3 text-sm leading-6 text-base-content/70">
        MusicBrainz highest-confidence matches will fill empty tags only. Existing tag values are preserved. Completed writes cannot be undone by Fika Music.
      </p>
      <div class="modal-action">
        <button class="btn" type="button" @click="metadataConfirmOpen = false">Cancel</button>
        <button class="btn btn-neutral" type="button" @click="startMetadataLookup">Start</button>
      </div>
    </div>
    <button class="modal-backdrop" type="button" aria-label="Cancel metadata lookup" @click="metadataConfirmOpen = false"></button>
  </div>

  <div v-if="propertiesTrack" class="modal modal-open" role="dialog" aria-modal="true" aria-label="Track properties">
    <div class="modal-box max-w-2xl rounded">
      <div class="flex items-start gap-3">
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold" :title="propertiesTrack.title">{{ propertiesTrack.title }}</h2>
          <p class="mt-0.5 truncate text-xs text-base-content/60" :title="propertiesTrack.filePath">{{ propertiesTrack.filePath }}</p>
        </div>
        <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Close properties" title="Close" @click="propertiesTrack = null"><X :size="16" aria-hidden="true" /></button>
      </div>
      <dl class="mt-5 grid grid-cols-[8rem_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
        <template
          v-for="entry in [
            ['Artist', propertiesTrack.artist || 'Unknown artist'],
            ['Album', propertiesTrack.album || 'Unknown album'],
            ['Album artist', propertiesTrack.albumArtist || ''],
            ['Genre', propertiesTrack.genre || ''],
            ['Year', propertiesTrack.year?.toString() || ''],
            ['Track / disc', `${propertiesTrack.trackNumber ?? '-'} / ${propertiesTrack.discNumber ?? '-'}`],
            ['Duration', formatDuration(propertiesTrack.durationSeconds)],
            ['Format', [propertiesTrack.codec, propertiesTrack.bitrateKbps ? `${propertiesTrack.bitrateKbps} kbps` : '', propertiesTrack.sampleRateHz ? formatSampleRate(propertiesTrack.sampleRateHz) : ''].filter(Boolean).join(' · ')],
            ['File size', formatFileSize(propertiesTrack.fileSizeBytes)],
            ['Play count', propertiesTrack.playCount.toLocaleString()],
          ]"
          :key="entry[0]"
        >
          <dt class="text-base-content/55">{{ entry[0] }}</dt>
          <dd class="min-w-0 break-words">{{ entry[1] || '—' }}</dd>
        </template>
      </dl>
    </div>
    <button class="modal-backdrop" type="button" aria-label="Close properties" @click="propertiesTrack = null"></button>
  </div>
</template>
