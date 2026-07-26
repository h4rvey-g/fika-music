import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DesktopLyricsWindow from "./DesktopLyricsWindow.vue";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_READY_EVENT,
  DESKTOP_LYRICS_STATE_EVENT,
  DESKTOP_LYRICS_UPDATE_EVENT,
} from "../lib/desktop-lyrics";

const tauriMocks = vi.hoisted(() => ({
  emitTo: vi.fn().mockResolvedValue(undefined),
  listen: vi.fn(),
  onCloseRequested: vi.fn().mockResolvedValue(vi.fn()),
  startDragging: vi.fn().mockResolvedValue(undefined),
}));
const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/event", () => ({
  emitTo: tauriMocks.emitTo,
  listen: tauriMocks.listen,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: tauriMocks.onCloseRequested,
    startDragging: tauriMocks.startDragging,
  }),
}));

describe("DesktopLyricsWindow", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    listeners.clear();
    tauriMocks.listen.mockImplementation(
      (event: string, handler: (event: { payload: unknown }) => void) => {
        listeners.set(event, handler);
        return Promise.resolve(vi.fn());
      },
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    document.documentElement.classList.remove("desktop-lyrics-root");
  });

  it("announces readiness and renders state broadcasts", async () => {
    const wrapper = mount(DesktopLyricsWindow);
    await flushPromises();

    expect(tauriMocks.emitTo).toHaveBeenCalledWith("main", DESKTOP_LYRICS_READY_EVENT);

    listeners.get(DESKTOP_LYRICS_STATE_EVENT)?.({
      payload: {
        title: "Track",
        subtitle: "Artist",
        currentLine: "Current lyric",
        currentLineKey: "timed:0:1000",
        currentLineStartMs: 1_000,
        currentLineEndMs: 2_000,
        currentWords: [
          { text: "Current ", startMs: 1_000, endMs: 1_500 },
          { text: "lyric", startMs: 1_500, endMs: 2_000 },
        ],
        currentTimingSource: "source",
        nextLine: "Next lyric",
        isPlaying: true,
        clockRunning: true,
        playbackRate: 1,
        playbackPositionMs: 1_250,
        preferences: {
          ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
          activeColor: "#22cc88",
        },
      },
    });
    await wrapper.vm.$nextTick();

    expect(wrapper.get('[data-testid="desktop-lyric-current"]').text()).toBe("Current lyric");
    expect(wrapper.get('[data-testid="desktop-lyric-next"]').text()).toBe("Next lyric");
    expect(wrapper.get('[data-testid="desktop-lyric-current"]').attributes("data-timing-source"))
      .toBe("source");
    expect(wrapper.findAll(".desktop-lyric-word")).toHaveLength(2);
    expect(wrapper.findAll(".desktop-lyric-word")[0].attributes("style"))
      .toContain("rgb(34, 204, 136)");
    expect(wrapper.get("main").classes()).toContain("border-transparent");
    expect(wrapper.get("main").classes()).toContain("hover:border-base-content/30");
    expect(wrapper.find('button[aria-label="Resize desktop lyrics window"]').exists())
      .toBe(false);
    wrapper.unmount();
  });

  it("advances word progress locally and freezes while paused", async () => {
    const wrapper = mount(DesktopLyricsWindow);
    await flushPromises();

    listeners.get(DESKTOP_LYRICS_STATE_EVENT)?.({
      payload: {
        title: "Track",
        subtitle: "Artist",
        currentLine: "AB",
        currentLineKey: "timed:0:1000",
        currentLineStartMs: 1_000,
        currentLineEndMs: 2_000,
        currentWords: [{ text: "AB", startMs: 1_000, endMs: 2_000 }],
        currentTimingSource: "source",
        nextLine: null,
        isPlaying: true,
        clockRunning: true,
        playbackRate: 1,
        playbackPositionMs: 1_000,
        preferences: { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES },
      },
    });
    await wrapper.vm.$nextTick();

    vi.advanceTimersByTime(500);
    await wrapper.vm.$nextTick();
    expect(Number(wrapper.get(".desktop-lyric-word").attributes("data-progress")))
      .toBeCloseTo(0.5, 1);

    listeners.get(DESKTOP_LYRICS_STATE_EVENT)?.({
      payload: {
        title: "Track",
        subtitle: "Artist",
        currentLine: "AB",
        currentLineKey: "timed:0:1000",
        currentLineStartMs: 1_000,
        currentLineEndMs: 2_000,
        currentWords: [{ text: "AB", startMs: 1_000, endMs: 2_000 }],
        currentTimingSource: "source",
        nextLine: null,
        isPlaying: false,
        clockRunning: false,
        playbackRate: 1,
        playbackPositionMs: 1_500,
        preferences: { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES },
      },
    });
    vi.advanceTimersByTime(300);
    await wrapper.vm.$nextTick();
    expect(Number(wrapper.get(".desktop-lyric-word").attributes("data-progress")))
      .toBeCloseTo(0.5, 2);
    wrapper.unmount();
  });

  it("applies shadow once to the composed lyric instead of every timed character", async () => {
    const wrapper = mount(DesktopLyricsWindow);
    await flushPromises();

    listeners.get(DESKTOP_LYRICS_STATE_EVENT)?.({
      payload: {
        title: "Track",
        subtitle: "Artist",
        currentLine: "AB",
        currentLineKey: "timed:0:1000",
        currentLineStartMs: 1_000,
        currentLineEndMs: 2_000,
        currentWords: [
          { text: "A", startMs: 1_000, endMs: 1_500 },
          { text: "B", startMs: 1_500, endMs: 2_000 },
        ],
        currentTimingSource: "source",
        nextLine: "Next lyric",
        isPlaying: true,
        clockRunning: false,
        playbackRate: 1,
        playbackPositionMs: 1_250,
        preferences: {
          ...DEFAULT_DESKTOP_LYRICS_PREFERENCES,
          effect: "shadow",
        },
      },
    });
    await wrapper.vm.$nextTick();

    const effectLayer = wrapper.get('[data-testid="desktop-lyric-current-effect"]');
    expect(effectLayer.attributes("data-text-effect")).toBe("shadow");
    expect(effectLayer.classes()).toContain("desktop-lyric-text-effect-shadow");
    for (const word of wrapper.findAll(".desktop-lyric-word")) {
      expect(word.attributes("style")).not.toContain("text-shadow");
      expect(word.attributes("style")).not.toContain("drop-shadow");
    }
    wrapper.unmount();
  });

  it("sends toolbar style changes back to the main window", async () => {
    const wrapper = mount(DesktopLyricsWindow);
    await flushPromises();
    tauriMocks.emitTo.mockClear();

    await wrapper.get('button[aria-label="Increase desktop lyric size"]').trigger("click");

    expect(tauriMocks.emitTo).toHaveBeenCalledWith(
      "main",
      DESKTOP_LYRICS_UPDATE_EVENT,
      { fontSize: DEFAULT_DESKTOP_LYRICS_PREFERENCES.fontSize + 2 },
    );
    wrapper.unmount();
  });
});
