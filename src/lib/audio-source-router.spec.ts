import { describe, expect, it } from "vitest";
import type { AudioSourceRecord } from "../generated/bindings";
import {
  AudioSourceRouter,
  playbackAttemptKey,
} from "./audio-source-router";
import {
  createAudioSourceRecord,
  createOnlineTrack,
  createOnlineTrackCandidate,
} from "../test/fixtures";

function audioSource(id: string): AudioSourceRecord {
  return createAudioSourceRecord({
    id,
    name: id,
    sources: [{
      id: "wy",
      name: "NetEase",
      type: "music",
      actions: ["musicUrl"],
      qualities: ["320k"],
    }],
  });
}

const track = createOnlineTrack({
  candidates: [createOnlineTrackCandidate()],
});

describe("AudioSourceRouter", () => {
  it("prefers the last fast successful source in automatic mode", () => {
    let now = 1_000;
    const router = new AudioSourceRouter(() => now);
    const first = audioSource("first");
    const second = audioSource("second");
    router.reportSuccess(playbackAttemptKey("second", "netease", "320k"), 180);
    now += 100;

    const ordered = router.order({
      records: [first, second],
      track,
      qualities: ["320k"],
      mode: "automatic",
      configuredPriority: ["first", "second"],
      selectedAudioSourceId: "first",
    });

    expect(ordered.map((source) => source.id)).toEqual(["second", "first"]);
  });

  it("temporarily deprioritizes a route after consecutive failures", () => {
    let now = 1_000;
    const router = new AudioSourceRouter(() => now);
    const key = playbackAttemptKey("first", "netease", "320k");
    router.reportSuccess(key, 100);
    router.reportFailure(key);
    router.reportFailure(key);

    expect(router.isAttemptAvailable(key)).toBe(false);
    now += 30_001;
    expect(router.isAttemptAvailable(key)).toBe(true);
  });

  it("selects only the earliest ejected route for recovery probing", () => {
    let now = 1_000;
    const router = new AudioSourceRouter(() => now);
    const first = playbackAttemptKey("first", "netease", "320k");
    const second = playbackAttemptKey("second", "netease", "320k");
    router.reportFailure(first);
    router.reportFailure(first);
    now += 1_000;
    router.reportFailure(second);
    router.reportFailure(second);

    expect(router.recoveryAttempt([second, first])).toBe(first);
  });

  it("keeps manual selection ahead of learned health", () => {
    const router = new AudioSourceRouter(() => 1_000);
    const first = audioSource("first");
    const second = audioSource("second");
    router.reportSuccess(playbackAttemptKey("second", "netease", "320k"), 100);

    const ordered = router.order({
      records: [first, second],
      track,
      qualities: ["320k"],
      mode: "manual",
      configuredPriority: ["second"],
      selectedAudioSourceId: "first",
    });

    expect(ordered.map((source) => source.id)).toEqual(["first", "second"]);
  });
});
