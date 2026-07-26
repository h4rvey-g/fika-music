<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  GripHorizontal,
  Lock,
  Minus,
  MonitorUp,
  Plus,
  Scaling,
  X,
} from "@lucide/vue";
import DesktopLyricTextEffect from "./DesktopLyricTextEffect.vue";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_HIDE_EVENT,
  DESKTOP_LYRICS_READY_EVENT,
  DESKTOP_LYRICS_STATE_EVENT,
  DESKTOP_LYRICS_UPDATE_EVENT,
  DESKTOP_LYRICS_WINDOW_LABEL,
  desktopLyricsMinimumHeight,
  parseDesktopLyricsPreferences,
  type DesktopLyricWordTiming,
  type DesktopLyricsPreferences,
  type DesktopLyricsState,
} from "../lib/desktop-lyrics";

const state = ref<DesktopLyricsState>({
  title: "Fika Music",
  subtitle: "",
  currentLine: "Nothing playing",
  currentLineKey: "message:Nothing playing",
  currentLineStartMs: null,
  currentLineEndMs: null,
  currentWords: [],
  currentTimingSource: null,
  nextLine: null,
  isPlaying: false,
  clockRunning: false,
  playbackRate: 1,
  playbackPositionMs: 0,
  preferences: { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES },
});
const renderedPositionMs = ref(0);
let positionReceivedAt = performance.now();
const unlisteners: UnlistenFn[] = [];
let animationFrameId: number | null = null;

const shellStyle = computed(() => ({
  backgroundColor: hexToRgba(
    state.value.preferences.backgroundColor,
    state.value.preferences.backgroundOpacity,
  ),
  borderColor: hexToRgba(state.value.preferences.inactiveColor, 0.18),
  fontFamily: fontFamily(state.value.preferences.font),
  textAlign: state.value.preferences.alignment,
}));

const currentLineStyle = computed(() => ({
  fontSize: `${state.value.preferences.fontSize}px`,
  fontWeight: state.value.preferences.fontWeight,
}));

const nextLineStyle = computed(() => ({
  color: state.value.preferences.inactiveColor,
  fontSize: `${Math.max(14, Math.round(state.value.preferences.fontSize * 0.52))}px`,
  fontWeight: Math.min(state.value.preferences.fontWeight, 600),
}));

onMounted(async () => {
  document.documentElement.classList.add("desktop-lyrics-root");
  startAnimationClock();
  unlisteners.push(
    await listen<DesktopLyricsState>(DESKTOP_LYRICS_STATE_EVENT, (event) => {
      state.value = {
        ...event.payload,
        currentLineKey: event.payload.currentLineKey
          ?? `legacy:${event.payload.currentLine}`,
        currentLineStartMs: event.payload.currentLineStartMs ?? null,
        currentLineEndMs: event.payload.currentLineEndMs ?? null,
        currentWords: event.payload.currentWords ?? [],
        currentTimingSource: event.payload.currentTimingSource ?? null,
        clockRunning: event.payload.clockRunning ?? event.payload.isPlaying,
        playbackRate: event.payload.playbackRate ?? 1,
        playbackPositionMs: event.payload.playbackPositionMs ?? 0,
        preferences: parseDesktopLyricsPreferences(event.payload.preferences),
      };
      positionReceivedAt = performance.now();
      renderedPositionMs.value = state.value.playbackPositionMs;
    }),
  );
  unlisteners.push(
    await getCurrentWindow().onCloseRequested((event) => {
      event.preventDefault();
      void requestHide();
    }),
  );
  await emitTo("main", DESKTOP_LYRICS_READY_EVENT);
});

onBeforeUnmount(() => {
  document.documentElement.classList.remove("desktop-lyrics-root");
  if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
  for (const unlisten of unlisteners) unlisten();
});

function startAnimationClock() {
  const update = (now: number) => {
    renderedPositionMs.value = estimatedPlaybackPosition(now);
    animationFrameId = requestAnimationFrame(update);
  };
  animationFrameId = requestAnimationFrame(update);
}

function estimatedPlaybackPosition(now: number) {
  const anchorPosition = Math.max(0, state.value.playbackPositionMs);
  if (!state.value.clockRunning) return anchorPosition;
  const estimated = anchorPosition
    + Math.max(0, now - positionReceivedAt) * Math.max(0, state.value.playbackRate);
  return state.value.currentLineEndMs === null
    ? estimated
    : Math.min(estimated, state.value.currentLineEndMs);
}

function wordProgress(word: DesktopLyricWordTiming) {
  if (word.isTimed === false) return 0;
  if (word.endMs <= word.startMs) {
    return renderedPositionMs.value >= word.endMs ? 1 : 0;
  }
  return Math.min(
    1,
    Math.max(0, (renderedPositionMs.value - word.startMs) / (word.endMs - word.startMs)),
  );
}

function wordStyle(word: DesktopLyricWordTiming) {
  const progress = Math.round(wordProgress(word) * 1000) / 10;
  return {
    backgroundImage: `linear-gradient(90deg, ${state.value.preferences.activeColor} 0%, ${state.value.preferences.activeColor} ${progress}%, ${state.value.preferences.inactiveColor} ${progress}%, ${state.value.preferences.inactiveColor} 100%)`,
  };
}

async function startDragging() {
  if (state.value.preferences.locked) return;
  try {
    await getCurrentWindow().startDragging();
  } catch {
    // The native drag API is unavailable in a regular browser preview.
  }
}

async function startResize(event: PointerEvent) {
  if (state.value.preferences.locked) return;
  const appWindow = getCurrentWindow();
  try {
    await appWindow.startResizeDragging("SouthEast");
  } catch {
    // macOS does not expose native resize dragging for borderless windows.
    try {
      const scaleFactor = await appWindow.scaleFactor();
      const initialSize = (await appWindow.innerSize()).toLogical(scaleFactor);
      const initialX = event.screenX;
      const initialY = event.screenY;
      const target = event.currentTarget as HTMLElement;
      target.setPointerCapture?.(event.pointerId);

      const resize = (moveEvent: PointerEvent) => {
        const width = Math.max(320, initialSize.width + moveEvent.screenX - initialX);
        const height = Math.max(
          desktopLyricsMinimumHeight(state.value.preferences),
          initialSize.height + moveEvent.screenY - initialY,
        );
        void appWindow.setSize(new LogicalSize(width, height));
      };
      const stop = () => {
        target.removeEventListener("pointermove", resize);
        target.removeEventListener("pointerup", stop);
        target.removeEventListener("pointercancel", stop);
      };
      target.addEventListener("pointermove", resize);
      target.addEventListener("pointerup", stop);
      target.addEventListener("pointercancel", stop);
    } catch {
      // The native window API is unavailable in a regular browser preview.
    }
  }
}

async function requestHide() {
  await emitTo("main", DESKTOP_LYRICS_HIDE_EVENT);
}

async function updatePreferences(patch: Partial<DesktopLyricsPreferences>) {
  state.value = {
    ...state.value,
    preferences: parseDesktopLyricsPreferences({ ...state.value.preferences, ...patch }),
  };
  await emitTo("main", DESKTOP_LYRICS_UPDATE_EVENT, patch);
}

function adjustFontSize(delta: number) {
  void updatePreferences({ fontSize: state.value.preferences.fontSize + delta });
}

function hexToRgba(color: string, opacity: number) {
  const red = Number.parseInt(color.slice(1, 3), 16);
  const green = Number.parseInt(color.slice(3, 5), 16);
  const blue = Number.parseInt(color.slice(5, 7), 16);
  return `rgba(${red}, ${green}, ${blue}, ${opacity})`;
}

function fontFamily(font: DesktopLyricsPreferences["font"]) {
  switch (font) {
    case "sans":
      return "Arial, Helvetica, sans-serif";
    case "serif":
      return "Georgia, 'Times New Roman', serif";
    case "rounded":
      return "ui-rounded, 'SF Pro Rounded', 'Segoe UI Rounded', sans-serif";
    case "monospace":
      return "ui-monospace, 'SFMono-Regular', Consolas, monospace";
    default:
      return "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif";
  }
}

</script>

<template>
  <main
    class="group relative flex size-full min-h-0 select-none flex-col justify-center overflow-hidden border"
    :class="{ 'opacity-80': !state.isPlaying }"
    :style="shellStyle"
    :data-window-label="DESKTOP_LYRICS_WINDOW_LABEL"
    @mousedown.left="startDragging"
  >
    <div
      v-if="!state.preferences.locked"
      class="z-20 flex h-9 shrink-0 items-center bg-black/55 px-1.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"
      @mousedown.stop="startDragging"
    >
      <div class="min-w-0 flex-1 truncate px-1.5 text-xs text-white/75">
        {{ state.title }}{{ state.subtitle ? ` · ${state.subtitle}` : "" }}
      </div>
      <GripHorizontal class="mx-1 shrink-0 text-white/45" :size="17" aria-hidden="true" />
      <div class="flex shrink-0 items-center gap-0.5">
        <div class="tooltip tooltip-bottom" data-tip="Smaller text">
          <button
            class="btn btn-square btn-ghost btn-xs text-white hover:bg-white/15"
            type="button"
            aria-label="Decrease desktop lyric size"
            title="Smaller text"
            @mousedown.stop
            @click.stop="adjustFontSize(-2)"
          >
            <Minus :size="14" aria-hidden="true" />
          </button>
        </div>
        <div class="tooltip tooltip-bottom" data-tip="Larger text">
          <button
            class="btn btn-square btn-ghost btn-xs text-white hover:bg-white/15"
            type="button"
            aria-label="Increase desktop lyric size"
            title="Larger text"
            @mousedown.stop
            @click.stop="adjustFontSize(2)"
          >
            <Plus :size="14" aria-hidden="true" />
          </button>
        </div>
        <div class="tooltip tooltip-bottom" data-tip="Always on top">
          <button
            class="btn btn-square btn-ghost btn-xs text-white hover:bg-white/15"
            :class="{ 'bg-white/15': state.preferences.alwaysOnTop }"
            type="button"
            aria-label="Toggle desktop lyrics always on top"
            :aria-pressed="state.preferences.alwaysOnTop"
            title="Always on top"
            @mousedown.stop
            @click.stop="updatePreferences({ alwaysOnTop: !state.preferences.alwaysOnTop })"
          >
            <MonitorUp :size="14" aria-hidden="true" />
          </button>
        </div>
        <div class="tooltip tooltip-bottom" data-tip="Lock window">
          <button
            class="btn btn-square btn-ghost btn-xs text-white hover:bg-white/15"
            type="button"
            aria-label="Lock desktop lyrics"
            title="Lock window"
            @mousedown.stop
            @click.stop="updatePreferences({ locked: true })"
          >
            <Lock :size="14" aria-hidden="true" />
          </button>
        </div>
        <div class="tooltip tooltip-bottom" data-tip="Resize">
          <button
            class="btn btn-square btn-ghost btn-xs cursor-nwse-resize text-white hover:bg-white/15"
            type="button"
            aria-label="Resize desktop lyrics window"
            title="Resize"
            @mousedown.stop
            @pointerdown.stop.prevent="startResize"
          >
            <Scaling :size="14" aria-hidden="true" />
          </button>
        </div>
        <div class="tooltip tooltip-bottom" data-tip="Hide">
          <button
            class="btn btn-square btn-ghost btn-xs text-white hover:bg-white/15"
            type="button"
            aria-label="Hide desktop lyrics"
            title="Hide"
            @mousedown.stop
            @click.stop="requestHide"
          >
            <X :size="14" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>

    <div class="flex min-h-0 w-full flex-1 flex-col justify-center overflow-hidden px-5 py-2" aria-live="polite">
      <div class="grid min-h-[2.24em] items-center">
        <Transition name="desktop-lyric-line">
          <div
            :key="state.currentLineKey"
            class="[grid-area:1/1] [overflow-wrap:anywhere] leading-[1.12] transition-[font-size] duration-200"
            :style="currentLineStyle"
            data-testid="desktop-lyric-current"
            :data-timing-source="state.currentTimingSource ?? 'none'"
          >
            <DesktopLyricTextEffect
              :effect="state.preferences.effect"
              :text="state.currentLine"
              :text-color="state.preferences.activeColor"
              :lines="2"
              data-testid="desktop-lyric-current-effect"
            >
              <template v-if="state.currentWords.length">
                <span
                  v-for="(word, index) in state.currentWords"
                  :key="`${index}:${word.startMs}:${word.text}`"
                  class="desktop-lyric-word"
                  :style="wordStyle(word)"
                  :data-progress="wordProgress(word).toFixed(3)"
                >{{ word.text }}</span>
              </template>
              <span v-else :style="{ color: state.preferences.activeColor }">
                {{ state.currentLine }}
              </span>
            </DesktopLyricTextEffect>
          </div>
        </Transition>
      </div>
      <div
        v-if="state.preferences.showNextLine && state.nextLine"
        class="mt-2 truncate leading-tight opacity-85 transition-colors duration-200"
        :style="nextLineStyle"
        data-testid="desktop-lyric-next"
      >
        <DesktopLyricTextEffect
          :effect="state.preferences.effect"
          :text="state.nextLine"
          :text-color="state.preferences.inactiveColor"
          :lines="1"
        />
      </div>
    </div>

  </main>
</template>

<style scoped>
.desktop-lyric-word {
  color: transparent;
  background-clip: text;
  -webkit-background-clip: text;
  white-space: pre-wrap;
}

.desktop-lyric-line-enter-active,
.desktop-lyric-line-leave-active {
  transition: opacity 260ms ease, transform 320ms cubic-bezier(0.22, 1, 0.36, 1), filter 260ms ease;
}

.desktop-lyric-line-enter-from {
  opacity: 0;
  transform: translateY(0.5em);
  filter: blur(2px);
}

.desktop-lyric-line-leave-to {
  opacity: 0;
  transform: translateY(-0.4em);
  filter: blur(1px);
}

@media (prefers-reduced-motion: reduce) {
  .desktop-lyric-line-enter-active,
  .desktop-lyric-line-leave-active {
    transition-duration: 1ms;
  }
}
</style>
