import { describe, expect, it } from "vitest";
import { PlayCountTracker } from "./play-count-tracker";

describe("PlayCountTracker", () => {
  it("records once after half the duration has actually played", () => {
    const tracker = new PlayCountTracker();
    tracker.start(1_000);

    const beforeThreshold = tracker.sample(50_000, 100);
    const atThreshold = tracker.sample(51_000, 100);
    const afterThreshold = tracker.sample(90_000, 100);

    expect([beforeThreshold, atThreshold, afterThreshold]).toEqual([false, true, false]);
  });

  it("does not count time spent paused", () => {
    const tracker = new PlayCountTracker();
    tracker.start(0);
    tracker.pause(20_000, 100);
    tracker.start(80_000);

    expect(tracker.sample(109_000, 100)).toBe(false);
  });

  it("does not use the media seek position", () => {
    const tracker = new PlayCountTracker();
    tracker.start(0);

    expect(tracker.sample(1_000, 100)).toBe(false);
  });
});
