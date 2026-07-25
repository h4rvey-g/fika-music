import { describe, expect, it } from "vitest";
import { normalizeError, queryError } from "./errors";

describe("normalizeError", () => {
  it.each([
    ["structured Tauri JSON", '{"message":"request failed"}', "fallback", "request failed"],
    ["plain strings", "request failed", "fallback", "request failed"],
    ["unknown values", null, "Provider failed.", "Provider failed."],
  ])("normalizes %s", (_case, error, fallback, expected) => {
    expect(normalizeError(error, fallback)).toBe(expected);
  });
});

describe("queryError", () => {
  it("prefixes active query failures", () => {
    expect(queryError("Playlist", true, new Error("expired"))).toBe("Playlist: expired");
  });
});
