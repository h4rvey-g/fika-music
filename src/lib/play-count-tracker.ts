export class PlayCountTracker {
  private listenedSeconds = 0;
  private startedAtMs: number | null = null;
  private recorded = false;

  reset() {
    this.listenedSeconds = 0;
    this.startedAtMs = null;
    this.recorded = false;
  }

  start(nowMs: number) {
    if (this.startedAtMs === null) {
      this.startedAtMs = nowMs;
    }
  }

  pause(nowMs: number, durationSeconds: number, playbackRate = 1) {
    const shouldRecord = this.sample(nowMs, durationSeconds, playbackRate);
    this.startedAtMs = null;
    return shouldRecord;
  }

  sample(nowMs: number, durationSeconds: number, playbackRate = 1) {
    if (this.startedAtMs === null || this.recorded) {
      return false;
    }
    const elapsedSeconds = Math.max(0, (nowMs - this.startedAtMs) / 1_000);
    this.listenedSeconds += elapsedSeconds * Math.max(0, playbackRate);
    this.startedAtMs = nowMs;
    if (durationSeconds <= 0 || this.listenedSeconds < durationSeconds * 0.5) {
      return false;
    }
    this.recorded = true;
    return true;
  }
}
