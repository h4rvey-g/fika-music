import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import OnlineCommentsDialog from "./OnlineCommentsDialog.vue";
import { createOnlineTrack, createOnlineTrackCandidate } from "../test/fixtures";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const track = createOnlineTrack({
  key: "comment-track",
  title: "Comment Song",
  artist: "Comment Artist",
  candidates: [
    createOnlineTrackCandidate({
      channelId: "fika.netease:wy",
      pluginId: "fika.netease",
      sourceId: "wy",
      id: "186016",
      platformIds: { id: "186016" },
    }),
    createOnlineTrackCandidate({
      channelId: "fika.kugou:kg",
      pluginId: "fika.kugou",
      sourceId: "kg",
      id: "4D766DEC7A90A011D730ED939D158131",
      platformIds: { mixSongId: 302362878 },
    }),
  ],
});

function comment(id: string, content: string) {
  return {
    id,
    userName: `Listener ${id}`,
    avatarUrl: null,
    content,
    timestampMs: null,
    timeLabel: "just now",
    likedCount: 3,
    replyCount: 1,
    location: null,
  };
}

function mountDialog() {
  return mount(OnlineCommentsDialog, {
    props: { track },
    global: { stubs: { Teleport: true } },
  });
}

describe("OnlineCommentsDialog", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command: string, args?: Record<string, any>) => {
      if (command === "cancel_source_request") return Promise.resolve(true);
      if (command !== "dispatch_plugin_request") return Promise.resolve(null);
      if (args?.pluginId === "fika.netease") {
        return Promise.resolve({
          response: {
            action: "musicComments",
            data: {
              hotComments: [comment("hot-1", "NetEase hot comment")],
              comments: [comment("netease-1", "NetEase comment")],
              total: 21,
              hasMore: false,
            },
          },
          diagnostics: [],
        });
      }
      const page = args?.request?.page ?? 1;
      return Promise.resolve({
        response: {
          action: "musicComments",
          data: {
            hotComments: [],
            comments: [comment(`kugou-${page}`, `KuGou page ${page}`)],
            total: 40,
            hasMore: page === 1,
          },
        },
        diagnostics: [],
      });
    });
  });

  it("loads the first source and switches providers lazily", async () => {
    const wrapper = mountDialog();
    await flushPromises();

    expect(wrapper.get("h2").text()).toBe("Comments");
    expect(wrapper.get(".badge").attributes("aria-label")).toBe("21 comments");
    expect(wrapper.text()).toContain("NetEase hot comment");
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "dispatch_plugin_request"),
    ).toHaveLength(1);

    await wrapper.get('[data-comment-source="fika.kugou"]').trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("KuGou page 1");
    expect(invokeMock).toHaveBeenLastCalledWith(
      "dispatch_plugin_request",
      expect.objectContaining({
        pluginId: "fika.kugou",
        request: expect.objectContaining({ action: "musicComments", page: 1 }),
      }),
    );
  });

  it("appends another comment page and closes from the icon button", async () => {
    const wrapper = mountDialog();
    await flushPromises();
    await wrapper.get('[data-comment-source="fika.kugou"]').trigger("click");
    await flushPromises();

    await wrapper.get("button.btn:not(.btn-square)").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("KuGou page 1");
    expect(wrapper.text()).toContain("KuGou page 2");
    await wrapper.get('button[aria-label="Close comments"]').trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("keeps loaded comments visible when the next page fails", async () => {
    const wrapper = mountDialog();
    await flushPromises();
    await wrapper.get('[data-comment-source="fika.kugou"]').trigger("click");
    await flushPromises();

    invokeMock.mockImplementation((command: string) => {
      if (command === "cancel_source_request") return Promise.resolve(true);
      if (command === "dispatch_plugin_request") return Promise.reject(new Error("network offline"));
      return Promise.resolve(null);
    });
    await wrapper.get("button.btn:not(.btn-square)").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("KuGou page 1");
    expect(wrapper.text()).toContain("network offline");
  });
});
