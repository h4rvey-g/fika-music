import type {
  AudioSourceRecord,
  AudioSourceSelectionMode,
  OnlineTrack,
  SourceQuality,
} from "../generated/bindings";

type AttemptHealth = {
  successes: number;
  failures: number;
  consecutiveFailures: number;
  ewmaLatencyMs: number | null;
  lastSuccessAt: number | null;
  lastObservedAt: number;
  ejectedUntil: number;
};

type RoutingOptions = {
  records: AudioSourceRecord[];
  track: OnlineTrack;
  qualities: SourceQuality[];
  mode: AudioSourceSelectionMode;
  configuredPriority: string[];
  selectedAudioSourceId?: string;
  preferredAudioSources?: Readonly<Record<string, string>>;
};

const HEALTH_TTL_MS = 30 * 60_000;
const RECENT_SUCCESS_MS = 10 * 60_000;
const BASE_EJECTION_MS = 30_000;
const MAX_EJECTION_MS = 5 * 60_000;
const UNKNOWN_ROUTE_SCORE = 2_500;
const SHARED_PREFERENCE_BONUS = 10_000;
const EWMA_ALPHA = 0.25;

export class AudioSourceRouter {
  private readonly health = new Map<string, AttemptHealth>();

  constructor(private readonly now: () => number = Date.now) {}

  order(options: RoutingOptions): AudioSourceRecord[] {
    this.prune();
    const enabled = options.records.filter(isEnabledPlaybackSource);
    const compatible = enabled.filter((record) =>
      sourceRoutes(record, options.track, options.qualities).length > 0
    );
    if (options.mode === "manual") {
      const order = [options.selectedAudioSourceId, ...options.configuredPriority]
        .filter((id): id is string => Boolean(id))
        .filter((id, index, values) => values.indexOf(id) === index);
      return [...compatible].sort((left, right) =>
        manualRank(left.id, order) - manualRank(right.id, order)
          || left.name.localeCompare(right.name)
      );
    }

    return [...compatible].sort((left, right) =>
      this.sourceScore(
        left,
        options.track,
        options.qualities,
        options.preferredAudioSources,
      )
        - this.sourceScore(
          right,
          options.track,
          options.qualities,
          options.preferredAudioSources,
        )
        || left.name.localeCompare(right.name)
    );
  }

  reportSuccess(attemptKey: string, latencyMs?: number): void {
    const now = this.now();
    const health = this.health.get(attemptKey) ?? emptyHealth(now);
    health.successes += 1;
    health.consecutiveFailures = 0;
    health.lastSuccessAt = now;
    health.lastObservedAt = now;
    health.ejectedUntil = 0;
    if (latencyMs !== undefined && Number.isFinite(latencyMs) && latencyMs >= 0) {
      const sample = Math.min(latencyMs, 30_000);
      health.ewmaLatencyMs = health.ewmaLatencyMs === null
        ? sample
        : EWMA_ALPHA * sample + (1 - EWMA_ALPHA) * health.ewmaLatencyMs;
    }
    this.health.set(attemptKey, health);
  }

  reportFailure(attemptKey: string): void {
    const now = this.now();
    const health = this.health.get(attemptKey) ?? emptyHealth(now);
    health.failures += 1;
    health.consecutiveFailures += 1;
    health.lastObservedAt = now;
    if (health.consecutiveFailures >= 2) {
      const multiplier = 2 ** Math.min(4, health.consecutiveFailures - 2);
      health.ejectedUntil = now + Math.min(
        MAX_EJECTION_MS,
        BASE_EJECTION_MS * multiplier,
      );
    }
    this.health.set(attemptKey, health);
  }

  hedgeDelayMs(
    source: AudioSourceRecord,
    track: OnlineTrack,
    qualities: SourceQuality[],
  ): number {
    const latencies = sourceRoutes(source, track, qualities)
      .map((route) => this.health.get(route.attemptKey)?.ewmaLatencyMs)
      .filter((latency): latency is number => latency !== null && latency !== undefined);
    const baseline = latencies.length ? Math.min(...latencies) : 900;
    return Math.round(Math.min(1_200, Math.max(400, baseline * 0.8)));
  }

  isAttemptAvailable(attemptKey: string): boolean {
    return (this.health.get(attemptKey)?.ejectedUntil ?? 0) <= this.now();
  }

  recoveryAttempt(attemptKeys: string[]): string | null {
    return attemptKeys.reduce<string | null>((earliest, attemptKey) => {
      if (earliest === null) return attemptKey;
      const currentUntil = this.health.get(attemptKey)?.ejectedUntil ?? 0;
      const earliestUntil = this.health.get(earliest)?.ejectedUntil ?? 0;
      return currentUntil < earliestUntil ? attemptKey : earliest;
    }, null);
  }

  reset(): void {
    this.health.clear();
  }

  private sourceScore(
    source: AudioSourceRecord,
    track: OnlineTrack,
    qualities: SourceQuality[],
    preferredAudioSources?: Readonly<Record<string, string>>,
  ): number {
    return Math.min(...sourceRoutes(source, track, qualities).map((route) => {
      const health = this.health.get(route.attemptKey);
      const routeBias = route.candidateIndex * 25 + route.qualityIndex * 75;
      const preferenceBias = preferredAudioSources?.[route.channelId] === source.id
        ? -SHARED_PREFERENCE_BONUS
        : 0;
      return (health ? this.healthScore(health) : UNKNOWN_ROUTE_SCORE)
        + routeBias
        + preferenceBias;
    }));
  }

  private healthScore(health: AttemptHealth): number {
    const now = this.now();
    if (health.ejectedUntil > now) {
      return 1_000_000 + health.ejectedUntil - now;
    }
    const observations = health.successes + health.failures;
    const successRate = (health.successes + 1) / (observations + 2);
    const latency = health.ewmaLatencyMs ?? 1_500;
    const failurePenalty = (1 - successRate) * 1_800
      + health.consecutiveFailures * 1_200;
    const successAge = health.lastSuccessAt === null ? RECENT_SUCCESS_MS : now - health.lastSuccessAt;
    const recencyBoost = 700 * Math.max(0, 1 - successAge / RECENT_SUCCESS_MS);
    return latency + failurePenalty - recencyBoost;
  }

  private prune(): void {
    const cutoff = this.now() - HEALTH_TTL_MS;
    for (const [key, health] of this.health) {
      if (health.lastObservedAt < cutoff) this.health.delete(key);
    }
  }
}

export function playbackAttemptKey(
  audioSourceId: string,
  channelId: string,
  quality: SourceQuality,
) {
  return `${audioSourceId}::${channelId}::${quality}`;
}

function sourceRoutes(
  source: AudioSourceRecord,
  track: OnlineTrack,
  qualities: SourceQuality[],
) {
  return track.candidates.flatMap((candidate, candidateIndex) => {
    const sourceInfo = source.sources.find((info) =>
      info.id === candidate.sourceId && info.actions.includes("musicUrl")
    );
    if (!sourceInfo) return [];
    return qualities.flatMap((quality, qualityIndex) =>
      !sourceInfo.qualities.length || sourceInfo.qualities.includes(quality) ? [{
        attemptKey: playbackAttemptKey(source.id, candidate.channelId, quality),
        channelId: candidate.channelId,
        candidateIndex,
        qualityIndex,
      }] : [],
    );
  });
}

function isEnabledPlaybackSource(record: AudioSourceRecord) {
  return record.enabled
    && record.state === "enabled"
    && record.sources.some((source) => source.actions.includes("musicUrl"));
}

function manualRank(id: string, order: string[]) {
  const index = order.indexOf(id);
  return index < 0 ? order.length : index;
}

function emptyHealth(now: number): AttemptHealth {
  return {
    successes: 0,
    failures: 0,
    consecutiveFailures: 0,
    ewmaLatencyMs: null,
    lastSuccessAt: null,
    lastObservedAt: now,
    ejectedUntil: 0,
  };
}
