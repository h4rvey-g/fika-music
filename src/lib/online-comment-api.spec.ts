import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRequestOutcome } from "../generated/bindings";
import { createOnlineTrack, createOnlineTrackCandidate } from "../test/fixtures";
import {
  getOnlineTrackComments,
  onlineTrackCommentSources,
  onlineTrackSupportsComments,
} from "./online-comment-api";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const neteaseCandidate = createOnlineTrackCandidate({
  channelId: "fika.netease:wy",
  pluginId: "fika.netease",
  sourceId: "wy",
  channelName: "NetEase Cloud Music",
  id: "186016",
  platformIds: { id: "186016" },
  rawInfo: { id: "186016" },
});
const kugouCandidate = createOnlineTrackCandidate({
  channelId: "fika.kugou:kg",
  pluginId: "fika.kugou",
  sourceId: "kg",
  channelName: "KuGou Music",
  id: "4D766DEC7A90A011D730ED939D158131",
  platformIds: { mixSongId: 302362878 },
  rawInfo: { hash: "4D766DEC7A90A011D730ED939D158131" },
});
const track = createOnlineTrack({ candidates: [kugouCandidate, neteaseCandidate] });

describe("online comment API", () => {
  beforeEach(() => invokeMock.mockReset());

  it("discovers supported comment sources in a stable provider order", () => {
    expect(onlineTrackCommentSources(track).map((source) => source.label)).toEqual([
      "NetEase",
      "KuGou",
    ]);
    expect(onlineTrackSupportsComments(track)).toBe(true);
  });

  it("dispatches normalized candidate metadata through musicComments", async () => {
    const response = {
      hotComments: [],
      comments: [],
      total: 0,
      hasMore: false,
    };
    invokeMock.mockResolvedValue({
      response: { action: "musicComments", data: response },
      diagnostics: [],
    } satisfies SourceRequestOutcome);
    const source = onlineTrackCommentSources(track)[1];

    await expect(getOnlineTrackComments(source, 2, 20, "comments-1")).resolves.toEqual(response);

    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.kugou",
      request: {
        action: "musicComments",
        source: "kg",
        musicInfo: {
          hash: "4D766DEC7A90A011D730ED939D158131",
          mixSongId: 302362878,
          id: "4D766DEC7A90A011D730ED939D158131",
          title: kugouCandidate.title,
          artist: kugouCandidate.artist,
        },
        page: 2,
        pageSize: 20,
      },
      requestId: "comments-1",
    });
  });
});
