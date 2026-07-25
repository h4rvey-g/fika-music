import { describe, expect, it } from "vitest";
import { normalizeError, queryError } from "./errors";

describe("normalizeError", () => {
  it("reads structured Tauri errors encoded as JSON", () => {
    expect(normalizeError('{"message":"request failed"}')).toBe("request failed");
  });

  it("preserves plain string errors", () => {
    expect(normalizeError("request failed")).toBe("request failed");
  });

  it("uses a caller-provided fallback for unknown values", () => {
    expect(normalizeError(null, "Provider failed.")).toBe("Provider failed.");
  });
});

describe("queryError", () => {
  it("prefixes active query failures", () => {
    expect(queryError("Playlist", true, new Error("expired"))).toBe("Playlist: expired");
  });
});
