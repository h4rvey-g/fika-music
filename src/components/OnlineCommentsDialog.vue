<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  AlertCircle,
  MessageCircle,
  RefreshCw,
  ThumbsUp,
  UserRound,
  X,
} from "@lucide/vue";
import type { OnlineTrack, SourceComment, SourceCommentsResponse } from "../generated/bindings";
import { normalizeError } from "../lib/errors";
import {
  getOnlineTrackComments,
  onlineTrackCommentSources,
  type OnlineTrackCommentSource,
} from "../lib/online-comment-api";
import { cancelSourceRequest } from "../lib/plugin-api";

type CommentState = {
  result: SourceCommentsResponse | null;
  page: number;
  loading: boolean;
  loadingMore: boolean;
  error: string | null;
  requestId: string | null;
  generation: number;
};

const PAGE_SIZE = 20;
const props = defineProps<{ track: OnlineTrack }>();
const emit = defineEmits<{ close: [] }>();
const sources = onlineTrackCommentSources(props.track);
const states = ref<Record<string, CommentState>>(
  Object.fromEntries(sources.map((source) => [source.pluginId, newCommentState()])),
);
const activePluginId = ref(sources[0]?.pluginId ?? "");
const activeSource = computed(() =>
  sources.find((source) => source.pluginId === activePluginId.value) ?? null
);
const activeState = computed(() =>
  activeSource.value ? states.value[activeSource.value.pluginId] : null
);

onMounted(() => {
  if (activeSource.value) void loadComments(activeSource.value, 1);
});

onBeforeUnmount(() => {
  for (const state of Object.values(states.value)) {
    state.generation += 1;
    if (state.requestId) void cancelSourceRequest(state.requestId);
  }
});

function newCommentState(): CommentState {
  return {
    result: null,
    page: 0,
    loading: false,
    loadingMore: false,
    error: null,
    requestId: null,
    generation: 0,
  };
}

function selectSource(source: OnlineTrackCommentSource) {
  activePluginId.value = source.pluginId;
  const state = states.value[source.pluginId];
  if (!state.result && !state.loading) void loadComments(source, 1);
}

async function loadComments(source: OnlineTrackCommentSource, page: number) {
  const state = states.value[source.pluginId];
  if (!state || state.loading || state.loadingMore) return;
  if (state.requestId) void cancelSourceRequest(state.requestId);
  const generation = ++state.generation;
  const requestId = `online-comments-${source.pluginId}-${Date.now()}-${generation}`;
  state.requestId = requestId;
  state.error = null;
  state.loading = page === 1;
  state.loadingMore = page > 1;
  try {
    const response = await getOnlineTrackComments(source, page, PAGE_SIZE, requestId);
    if (generation !== state.generation) return;
    state.result = page === 1 ? response : appendComments(state.result, response);
    state.page = page;
  } catch (error) {
    if (generation === state.generation) state.error = normalizeError(error);
  } finally {
    if (generation === state.generation) {
      state.loading = false;
      state.loadingMore = false;
      state.requestId = null;
    }
  }
}

function appendComments(
  current: SourceCommentsResponse | null,
  next: SourceCommentsResponse,
): SourceCommentsResponse {
  if (!current) return next;
  const ids = new Set(current.comments.map((comment) => comment.id));
  return {
    hotComments: current.hotComments,
    comments: [
      ...current.comments,
      ...next.comments.filter((comment) => !ids.has(comment.id)),
    ],
    total: next.total ?? current.total,
    hasMore: next.hasMore,
  };
}

function retryActiveSource() {
  if (activeSource.value) void loadComments(activeSource.value, 1);
}

function loadMore() {
  if (activeSource.value && activeState.value?.result?.hasMore) {
    void loadComments(activeSource.value, activeState.value.page + 1);
  }
}

function commentTime(comment: SourceComment) {
  if (comment.timeLabel) return comment.timeLabel;
  if (comment.timestampMs === null) return "";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(comment.timestampMs));
}

function compactCount(value: number) {
  return new Intl.NumberFormat(undefined, { notation: "compact" }).format(value);
}

function commentCountLabel(value: number) {
  return `${value.toLocaleString()} ${value === 1 ? "comment" : "comments"}`;
}
</script>

<template>
  <Teleport to="body">
    <dialog
      open
      tabindex="0"
      class="modal"
      aria-labelledby="online-comments-title"
      @cancel.prevent="emit('close')"
    >
      <div
        class="modal-box flex max-h-[min(88vh,48rem)] w-[min(56rem,calc(100vw-2rem))] max-w-4xl flex-col overflow-hidden p-0"
        data-online-comments-dialog
      >
        <header class="flex items-center gap-3 border-b border-base-300 px-5 py-4">
          <div class="flex size-11 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
            <img
              v-if="track.coverUrl"
              :src="track.coverUrl"
              class="size-full object-cover"
              alt=""
            />
            <MessageCircle v-else :size="20" aria-hidden="true" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 id="online-comments-title" class="truncate text-base font-semibold">Comments</h2>
            <p class="truncate text-sm text-muted">{{ track.title }} by {{ track.artist }}</p>
          </div>
          <span
            v-if="activeState?.result?.total !== null && activeState?.result?.total !== undefined"
            class="badge badge-ghost shrink-0"
            :aria-label="commentCountLabel(activeState.result.total)"
          >
            <span class="hidden sm:inline">{{ commentCountLabel(activeState.result.total) }}</span>
            <span class="sm:hidden" aria-hidden="true">
              {{ activeState.result.total.toLocaleString() }}
            </span>
          </span>
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            aria-label="Close comments"
            title="Close"
            @click="emit('close')"
          >
            <X :size="17" aria-hidden="true" />
          </button>
        </header>

        <div
          v-if="sources.length > 1"
          role="tablist"
          class="tabs tabs-border shrink-0 px-5 pt-2"
          aria-label="Comment sources"
        >
          <button
            v-for="source in sources"
            :key="source.pluginId"
            role="tab"
            class="tab gap-2"
            :class="activePluginId === source.pluginId ? 'tab-active' : ''"
            :aria-selected="activePluginId === source.pluginId"
            :data-comment-source="source.pluginId"
            type="button"
            @click="selectSource(source)"
          >
            {{ source.label }}
            <span
              v-if="states[source.pluginId].result?.total !== null && states[source.pluginId].result?.total !== undefined"
              class="text-xs text-muted"
            >
              {{ compactCount(states[source.pluginId].result?.total ?? 0) }}
            </span>
          </button>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
          <div v-if="activeState?.loading" class="grid min-h-64 place-items-center">
            <span class="loading loading-spinner loading-md" aria-label="Loading comments"></span>
          </div>

          <div
            v-else-if="activeState?.error && !activeState.result"
            role="alert"
            class="alert alert-error mt-4"
          >
            <AlertCircle :size="18" aria-hidden="true" />
            <span class="min-w-0 flex-1 text-sm">{{ activeState.error }}</span>
            <button class="btn btn-sm" type="button" @click="retryActiveSource">
              <RefreshCw :size="15" aria-hidden="true" />
              Retry
            </button>
          </div>

          <div
            v-else-if="activeState?.result && !activeState.result.hotComments.length && !activeState.result.comments.length"
            class="grid min-h-64 place-items-center text-sm text-muted"
          >
            No comments yet
          </div>

          <template v-else-if="activeState?.result">
            <section v-if="activeState.result.hotComments.length" class="pt-4">
              <h3 class="pb-1 text-xs font-semibold text-muted">Top comments</h3>
              <ul class="list divide-y divide-base-300" aria-label="Top comments">
                <li
                  v-for="comment in activeState.result.hotComments"
                  :key="`hot:${comment.id}`"
                  class="list-row px-0 py-3"
                >
                  <div class="avatar">
                    <div class="grid size-9 place-items-center overflow-hidden rounded-full bg-base-200">
                      <img v-if="comment.avatarUrl" :src="comment.avatarUrl" alt="" />
                      <UserRound v-else :size="17" aria-hidden="true" />
                    </div>
                  </div>
                  <div class="list-col-grow min-w-0">
                    <div class="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
                      <span class="truncate text-sm font-medium">{{ comment.userName }}</span>
                      <span class="text-xs text-muted">{{ commentTime(comment) }}</span>
                      <span v-if="comment.location" class="text-xs text-muted">{{ comment.location }}</span>
                    </div>
                    <p class="mt-1 whitespace-pre-wrap break-words text-sm leading-6">{{ comment.content }}</p>
                  </div>
                  <div class="flex shrink-0 items-center gap-3 text-xs text-muted">
                    <span class="inline-flex items-center gap-1">
                      <ThumbsUp :size="13" aria-hidden="true" />
                      {{ compactCount(comment.likedCount) }}
                    </span>
                    <span v-if="comment.replyCount" class="inline-flex items-center gap-1">
                      <MessageCircle :size="13" aria-hidden="true" />
                      {{ compactCount(comment.replyCount) }}
                    </span>
                  </div>
                </li>
              </ul>
            </section>

            <section v-if="activeState.result.comments.length" class="pt-4">
              <h3 class="pb-1 text-xs font-semibold text-muted">Recent comments</h3>
              <ul class="list divide-y divide-base-300" aria-label="Recent comments">
                <li
                  v-for="comment in activeState.result.comments"
                  :key="comment.id"
                  class="list-row px-0 py-3"
                >
                  <div class="avatar">
                    <div class="grid size-9 place-items-center overflow-hidden rounded-full bg-base-200">
                      <img v-if="comment.avatarUrl" :src="comment.avatarUrl" alt="" />
                      <UserRound v-else :size="17" aria-hidden="true" />
                    </div>
                  </div>
                  <div class="list-col-grow min-w-0">
                    <div class="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
                      <span class="truncate text-sm font-medium">{{ comment.userName }}</span>
                      <span class="text-xs text-muted">{{ commentTime(comment) }}</span>
                      <span v-if="comment.location" class="text-xs text-muted">{{ comment.location }}</span>
                    </div>
                    <p class="mt-1 whitespace-pre-wrap break-words text-sm leading-6">{{ comment.content }}</p>
                  </div>
                  <div class="flex shrink-0 items-center gap-3 text-xs text-muted">
                    <span class="inline-flex items-center gap-1">
                      <ThumbsUp :size="13" aria-hidden="true" />
                      {{ compactCount(comment.likedCount) }}
                    </span>
                    <span v-if="comment.replyCount" class="inline-flex items-center gap-1">
                      <MessageCircle :size="13" aria-hidden="true" />
                      {{ compactCount(comment.replyCount) }}
                    </span>
                  </div>
                </li>
              </ul>
            </section>

            <div v-if="activeState.error" role="alert" class="alert alert-error mt-4">
              <AlertCircle :size="18" aria-hidden="true" />
              <span class="min-w-0 flex-1 text-sm">{{ activeState.error }}</span>
              <button class="btn btn-sm" type="button" @click="loadMore">
                <RefreshCw :size="15" aria-hidden="true" />
                Retry
              </button>
            </div>

            <div
              v-else-if="activeState.result.hasMore"
              class="flex justify-center border-t border-base-300 pt-4"
            >
              <button
                class="btn btn-sm"
                type="button"
                :disabled="activeState.loadingMore"
                @click="loadMore"
              >
                <span
                  v-if="activeState.loadingMore"
                  class="loading loading-spinner loading-xs"
                  aria-hidden="true"
                ></span>
                <RefreshCw v-else :size="15" aria-hidden="true" />
                Load more
              </button>
            </div>
          </template>
        </div>
      </div>
      <form method="dialog" class="modal-backdrop" @submit.prevent="emit('close')">
        <button type="submit">Close</button>
      </form>
    </dialog>
  </Teleport>
</template>
