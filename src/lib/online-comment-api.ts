import type {
  OnlineTrack,
  OnlineTrackCandidate,
  SourceCommentsResponse,
} from "../generated/bindings";
import { KUGOU_PLUGIN_ID } from "./kugou-api";
import { NETEASE_PLUGIN_ID } from "./netease-api";
import { dispatchPluginRequest } from "./plugin-api";

export type OnlineTrackCommentSource = {
  pluginId: string;
  label: string;
  candidate: OnlineTrackCandidate;
};

const COMMENT_PROVIDERS = [
  { pluginId: NETEASE_PLUGIN_ID, label: "NetEase" },
  { pluginId: KUGOU_PLUGIN_ID, label: "KuGou" },
] as const;

export function onlineTrackCommentSources(track: OnlineTrack): OnlineTrackCommentSource[] {
  return COMMENT_PROVIDERS.flatMap((provider) => {
    const candidate = track.candidates.find((item) =>
      item.pluginId === provider.pluginId && hasCommentIdentity(item)
    );
    return candidate ? [{ ...provider, candidate }] : [];
  });
}

export function onlineTrackSupportsComments(track: OnlineTrack) {
  return onlineTrackCommentSources(track).length > 0;
}

export async function getOnlineTrackComments(
  source: OnlineTrackCommentSource,
  page: number,
  pageSize = 20,
  requestId?: string,
): Promise<SourceCommentsResponse> {
  const candidate = source.candidate;
  const outcome = await dispatchPluginRequest(
    source.pluginId,
    {
      action: "musicComments",
      source: candidate.sourceId,
      musicInfo: {
        ...candidate.rawInfo,
        ...candidate.platformIds,
        id: candidate.id,
        title: candidate.title,
        artist: candidate.artist,
      },
      page,
      pageSize,
    },
    requestId,
  );
  if (outcome.response.action !== "musicComments") {
    throw new Error(
      `${source.label} provider returned ${outcome.response.action} for musicComments`,
    );
  }
  return outcome.response.data;
}

function hasCommentIdentity(candidate: OnlineTrackCandidate) {
  if (candidate.pluginId === NETEASE_PLUGIN_ID) {
    return /^\d+$/u.test(candidate.id);
  }
  if (candidate.pluginId !== KUGOU_PLUGIN_ID) return false;
  const mixSongId = candidate.platformIds.mixSongId
    ?? candidate.platformIds.albumAudioId
    ?? candidate.rawInfo.mixSongId
    ?? candidate.rawInfo.albumAudioId;
  return positiveInteger(mixSongId);
}

function positiveInteger(value: unknown) {
  if (typeof value === "number") return Number.isSafeInteger(value) && value > 0;
  return typeof value === "string" && /^[1-9]\d*$/u.test(value);
}
