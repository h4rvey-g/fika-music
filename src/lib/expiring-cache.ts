type CacheEntry<Value> = {
  expiresAt: number;
  value: Value;
};

export class ExpiringCache<Key, Value> {
  private readonly entries = new Map<Key, CacheEntry<Value>>();

  constructor(
    private readonly ttlMs: number,
    private readonly maxEntries: number,
    private readonly now: () => number = Date.now,
  ) {
    if (ttlMs <= 0 || maxEntries <= 0) {
      throw new Error("ExpiringCache requires positive TTL and capacity values.");
    }
  }

  get(key: Key): Value | undefined {
    this.pruneExpired();
    const entry = this.entries.get(key);
    if (!entry) return undefined;

    this.entries.delete(key);
    this.entries.set(key, entry);
    return entry.value;
  }

  set(key: Key, value: Value): void {
    this.pruneExpired();
    this.entries.delete(key);
    this.entries.set(key, {
      expiresAt: this.now() + this.ttlMs,
      value,
    });
    while (this.entries.size > this.maxEntries) {
      const oldestKey = this.entries.keys().next().value as Key | undefined;
      if (oldestKey === undefined) break;
      this.entries.delete(oldestKey);
    }
  }

  delete(key: Key): void {
    this.entries.delete(key);
  }

  deleteWhere(predicate: (key: Key) => boolean): void {
    for (const key of this.entries.keys()) {
      if (predicate(key)) this.entries.delete(key);
    }
  }

  keysWhere(predicate: (key: Key) => boolean): Key[] {
    this.pruneExpired();
    return [...this.entries.keys()].filter(predicate);
  }

  clear(): void {
    this.entries.clear();
  }

  get size(): number {
    this.pruneExpired();
    return this.entries.size;
  }

  private pruneExpired(): void {
    const now = this.now();
    for (const [key, entry] of this.entries) {
      if (entry.expiresAt <= now) this.entries.delete(key);
    }
  }
}
