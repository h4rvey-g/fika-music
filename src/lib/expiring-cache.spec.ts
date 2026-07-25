import { describe, expect, it } from "vitest";
import { ExpiringCache } from "./expiring-cache";

describe("ExpiringCache", () => {
  it("prunes every expired entry during normal access", () => {
    let now = 0;
    const cache = new ExpiringCache<string, number>(100, 4, () => now);
    cache.set("first", 1);
    cache.set("second", 2);

    now = 101;

    expect(cache.get("missing")).toBeUndefined();
    expect(cache.size).toBe(0);
  });

  it("evicts the least recently used entry at capacity", () => {
    const cache = new ExpiringCache<string, number>(100, 2, () => 0);
    cache.set("first", 1);
    cache.set("second", 2);
    cache.get("first");
    cache.set("third", 3);

    expect(cache.get("second")).toBeUndefined();
    expect(cache.get("first")).toBe(1);
    expect(cache.get("third")).toBe(3);
  });

  it("deletes entries selected by their key", () => {
    const cache = new ExpiringCache<string, number>(100, 4, () => 0);
    cache.set("track-a::source", 1);
    cache.set("track-b::source", 2);

    cache.deleteWhere((key) => key.startsWith("track-a::"));

    expect(cache.get("track-a::source")).toBeUndefined();
    expect(cache.get("track-b::source")).toBe(2);
  });
});
