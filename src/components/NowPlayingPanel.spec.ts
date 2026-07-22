import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import NowPlayingPanel from "./NowPlayingPanel.vue";

describe("NowPlayingPanel", () => {
  it("shows the actual network lyric provider", () => {
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
          lines: [{ startMs: 0, text: "Lyric" }],
        },
      },
    });

    const sourceBadge = wrapper.get('[title="QQ Music #003aAYrm3GE0Ac"]');
    expect(sourceBadge.text()).toBe("QQ Music");
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
            { startMs: 1_000, text: "First line" },
            { startMs: 3_000, text: "Second line" },
          ],
        },
      },
    });

    expect(wrapper.get('[data-active="true"]').text()).toBe("First line");

    await wrapper.setProps({ playbackPosition: 3.5 });

    expect(wrapper.get('[data-active="true"]').text()).toBe("Second line");
  });

  it("emits a retry command from the lyrics toolbar", async () => {
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

    await wrapper.get('button[aria-label="Retry lyrics"]').trigger("click");

    expect(wrapper.emitted("retryLyrics")).toHaveLength(1);
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
            { startMs: 1_000, text: "First line" },
            { startMs: 3_000, text: "Second line" },
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
});
