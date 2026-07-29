import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import NowPlayingPanel from "./NowPlayingPanel.vue";
import { DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES } from "../lib/now-playing-lyrics";

function dispatchPointerEvent(
  element: Element,
  type: string,
  options: { pointerId: number; clientY?: number; button?: number },
) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    button: options.button ?? 0,
    clientY: options.clientY ?? 0,
  });
  Object.defineProperty(event, "pointerId", { value: options.pointerId });
  element.dispatchEvent(event);
}

describe("NowPlayingPanel", () => {
  it("omits the lyrics heading and source metadata", () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 0,
        canRetry: false,
        lyrics: {
          source: "network",
          provider: "QQ Music #003aAYrm3GE0Ac",
          isSynced: true,
          savedPath: null,
          matchScore: 100,
          lines: [{ startMs: 0, endMs: null, text: "Lyric", words: [] }],
        },
      },
    });

    expect(wrapper.find("h2").exists()).toBe(false);
    expect(wrapper.find('[title="QQ Music #003aAYrm3GE0Ac"]').exists()).toBe(false);
  });

  it("highlights the synchronized lyric line for the current playback position", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 1.5,
        canRetry: true,
        lyrics: {
          source: "network",
          provider: "LRCLIB #1",
          isSynced: true,
          savedPath: null,
          matchScore: 95,
          lines: [
            { startMs: 1_000, endMs: 3_000, text: "First line", words: [] },
            { startMs: 3_000, endMs: null, text: "Second line", words: [] },
          ],
        },
      },
    });

    expect(wrapper.get('[data-active="true"]').text()).toBe("First line");

    await wrapper.setProps({ playbackPosition: 3.5 });

    expect(wrapper.get('[data-active="true"]').text()).toBe("Second line");
  });

  it("offers retry from the lyrics context menu", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyrics: null,
        lyricsLoading: false,
        lyricsError: "Network unavailable",
        playbackPosition: 0,
        canRetry: true,
      },
    });

    expect(wrapper.find('button[aria-label="Retry lyrics"]').exists()).toBe(false);

    await wrapper.get('[data-testid="lyrics-viewport"]').trigger("contextmenu", {
      clientX: 100,
      clientY: 80,
    });
    await wrapper.vm.$nextTick();

    const menu = document.body.querySelector<HTMLElement>("[data-lyrics-context-menu]");
    const retryAction = Array.from(menu?.querySelectorAll("button") ?? [])
      .find((button) => button.textContent?.includes("Retry lyrics"));
    expect(retryAction).toBeDefined();
    expect(document.activeElement).toBe(retryAction);

    retryAction?.click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted("retryLyrics")).toHaveLength(1);
    expect(document.body.querySelector("[data-lyrics-context-menu]")).toBeNull();
    wrapper.unmount();
  });

  it("keeps synchronized lyric scrolling inside the lyrics viewport", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 1.5,
        canRetry: true,
        lyrics: {
          source: "embedded",
          provider: null,
          isSynced: true,
          savedPath: null,
          matchScore: null,
          lines: [
            { startMs: 1_000, endMs: 3_000, text: "First line", words: [] },
            { startMs: 3_000, endMs: null, text: "Second line", words: [] },
          ],
        },
      },
    });
    const viewport = wrapper.get('[data-testid="lyrics-viewport"]').element as HTMLElement;
    const secondLine = wrapper.get('[data-lyric-index="1"]').element as HTMLElement;
    const scrollTo = vi.fn();
    Object.defineProperties(viewport, {
      clientHeight: { configurable: true, value: 200 },
      scrollTo: { configurable: true, value: scrollTo },
    });
    Object.defineProperties(secondLine, {
      offsetHeight: { configurable: true, value: 40 },
      offsetTop: { configurable: true, value: 280 },
    });

    await wrapper.setProps({ playbackPosition: 3.5 });
    await wrapper.vm.$nextTick();

    expect(scrollTo).toHaveBeenCalledWith({ behavior: "smooth", top: 200 });
  });

  it("seeks to the synchronized lyric centered by a drag", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 1.5,
        canRetry: true,
        lyrics: {
          source: "embedded",
          provider: null,
          isSynced: true,
          savedPath: null,
          matchScore: null,
          lines: [
            { startMs: 1_000, endMs: 3_000, text: "First line", words: [] },
            { startMs: 3_000, endMs: 5_000, text: "Second line", words: [] },
            { startMs: 5_000, endMs: null, text: "Third line", words: [] },
          ],
        },
      },
    });
    const viewportWrapper = wrapper.get('[data-testid="lyrics-viewport"]');
    const viewport = viewportWrapper.element as HTMLElement;
    Object.defineProperties(viewport, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 600 },
      scrollTop: { configurable: true, writable: true, value: 0 },
      setPointerCapture: { configurable: true, value: vi.fn() },
      hasPointerCapture: { configurable: true, value: vi.fn(() => true) },
      releasePointerCapture: { configurable: true, value: vi.fn() },
    });
    wrapper.findAll('[data-lyric-index]').forEach((line, index) => {
      Object.defineProperties(line.element, {
        offsetHeight: { configurable: true, value: 40 },
        offsetTop: { configurable: true, value: 80 + index * 100 },
      });
    });

    dispatchPointerEvent(viewport, "pointerdown", {
      button: 0,
      clientY: 100,
      pointerId: 1,
    });
    dispatchPointerEvent(viewport, "pointermove", {
      clientY: -80,
      pointerId: 1,
    });
    await wrapper.vm.$nextTick();

    expect(viewport.scrollTop).toBe(180);
    expect(wrapper.find('[data-testid="lyric-seek-guide"]').exists()).toBe(true);
    expect(wrapper.get('[data-testid="lyric-seek-time"]').text()).toBe("0:05");
    expect(wrapper.get('[data-active="true"]').text()).toBe("Third line");

    dispatchPointerEvent(viewport, "pointerup", { pointerId: 1 });
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted("seekPlayback")).toEqual([[5]]);
    expect(wrapper.find('[data-testid="lyric-seek-guide"]').exists()).toBe(false);
  });

  it("does not seek when dragging unsynchronized lyrics", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 0,
        canRetry: false,
        lyrics: {
          source: "embedded",
          provider: null,
          isSynced: false,
          savedPath: null,
          matchScore: null,
          lines: [{ startMs: null, endMs: null, text: "Plain lyric", words: [] }],
        },
      },
    });
    const viewport = wrapper.get('[data-testid="lyrics-viewport"]');

    dispatchPointerEvent(viewport.element, "pointerdown", {
      button: 0,
      clientY: 100,
      pointerId: 1,
    });
    dispatchPointerEvent(viewport.element, "pointermove", { clientY: 20, pointerId: 1 });
    dispatchPointerEvent(viewport.element, "pointerup", { pointerId: 1 });
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted("seekPlayback")).toBeUndefined();
  });

  it("applies the configured lyric typography and colors", () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 1.5,
        canRetry: false,
        lyricsPreferences: {
          ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
          font: "serif",
          fontSize: 20,
          lineGap: 12,
          activeFontWeight: 700,
          alignment: "left",
          activeColor: "#123456",
          inactiveColor: "#abcdef",
          inactiveOpacity: 0.3,
        },
        lyrics: {
          source: "embedded",
          provider: null,
          isSynced: true,
          savedPath: null,
          matchScore: null,
          lines: [
            { startMs: 1_000, endMs: 3_000, text: "Current line", words: [] },
            { startMs: 3_000, endMs: null, text: "Other line", words: [] },
          ],
        },
      },
    });

    const textContainer = wrapper.get('[data-testid="lyric-lines"]').element as HTMLElement;
    const activeLine = wrapper.get('[data-active="true"]').element as HTMLElement;
    const inactiveLine = wrapper.get('[data-lyric-index="1"]').element as HTMLElement;

    expect(textContainer.style.fontFamily).toContain("Georgia");
    expect(textContainer.style.textAlign).toBe("left");
    expect(activeLine.style.color).toBe("rgb(18, 52, 86)");
    expect(activeLine.style.fontSize).toBe("20px");
    expect(activeLine.style.fontWeight).toBe("700");
    expect(activeLine.style.paddingBlock).toBe("6px");
    expect(inactiveLine.style.color).toBe("rgb(171, 205, 239)");
    expect(inactiveLine.style.opacity).toBe("0.3");
  });

  it("offers lyric appearance settings from the lyrics context menu", async () => {
    const wrapper = mount(NowPlayingPanel, {
      props: {
        title: "Track",
        subtitle: "Artist",
        coverUrl: null,
        lyrics: null,
        lyricsLoading: false,
        lyricsError: null,
        playbackPosition: 0,
        canRetry: false,
      },
    });

    await wrapper.get('[data-testid="lyrics-viewport"]').trigger("contextmenu", {
      clientX: 100,
      clientY: 80,
    });
    await wrapper.vm.$nextTick();

    const menu = document.body.querySelector<HTMLElement>("[data-lyrics-context-menu]");
    const action = menu?.querySelector<HTMLButtonElement>("button");
    expect(menu?.textContent).toContain("Lyrics appearance");
    expect(menu?.textContent).not.toContain("Retry lyrics");
    expect(document.activeElement).toBe(action);

    action?.click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted("openLyricsSettings")).toHaveLength(1);
    expect(document.body.querySelector("[data-lyrics-context-menu]")).toBeNull();
    wrapper.unmount();
  });
});
