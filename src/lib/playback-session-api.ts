import { invoke } from "@tauri-apps/api/core";
import {
  TAURI_COMMANDS,
  type LibraryPlaybackQueue,
  type LocalTrack,
  type OnlineTrack,
  type PlaybackMode,
  type PlaybackSessionSnapshot,
  type PlaybackTrackInput,
} from "../generated/bindings";

export const PLAYBACK_QUEUE_PAGE_SIZE = 200;

export function localPlaybackInput(track: LocalTrack): PlaybackTrackInput {
  return { kind: "local", trackId: track.id };
}

export function onlinePlaybackInput(track: OnlineTrack): PlaybackTrackInput {
  return { kind: "online", track };
}

export function getPlaybackSession(
  offset = 0,
  limit = PLAYBACK_QUEUE_PAGE_SIZE,
) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.getPlaybackSession, {
    offset,
    limit,
  });
}

export function replacePlaybackSession(
  tracks: PlaybackTrackInput[],
  startIndex: number,
  autoplay: boolean,
  mode: PlaybackMode,
  streamOpen = false,
) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.replacePlaybackSession, {
    tracks,
    startIndex,
    autoplay,
    mode,
    streamOpen,
  });
}

export function replaceLocalPlaybackSession(
  queue: LibraryPlaybackQueue,
  autoplay: boolean,
  mode: PlaybackMode,
) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.replaceLocalPlaybackSession, {
    queueId: queue.queueId,
    startIndex: queue.currentIndex,
    autoplay,
    mode,
  });
}

export function playNextInPlaybackSession(tracks: PlaybackTrackInput[]) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.playNextInPlaybackSession, { tracks });
}

export function appendPlaybackSessionStream(tracks: PlaybackTrackInput[]) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.appendPlaybackSessionStream, { tracks });
}

export function closePlaybackSessionStream() {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.closePlaybackSessionStream);
}

export function setPlaybackSessionMode(mode: PlaybackMode) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.setPlaybackSessionMode, { mode });
}

export function movePlaybackSessionItem(from: number, to: number) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.movePlaybackSessionItem, { from, to });
}

export function removePlaybackSessionItem(index: number) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.removePlaybackSessionItem, { index });
}

export function clearPlaybackSessionUpcoming() {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.clearPlaybackSessionUpcoming);
}

export function selectPlaybackSessionItem(index: number) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.selectPlaybackSessionItem, { index });
}

export function nextPlaybackSessionItem(manual = false) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.nextPlaybackSessionItem, { manual });
}

export function previousPlaybackSessionItem() {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.previousPlaybackSessionItem);
}

export function failPlaybackSessionCurrent() {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.failPlaybackSessionCurrent);
}

export function markPlaybackSessionStarted() {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.markPlaybackSessionStarted);
}

export function setPlaybackSessionPaused(paused: boolean) {
  return invoke<PlaybackSessionSnapshot>(TAURI_COMMANDS.setPlaybackSessionPaused, { paused });
}

export function savePlaybackSessionProgress(positionSeconds: number) {
  return invoke<void>(TAURI_COMMANDS.savePlaybackSessionProgress, { positionSeconds });
}
