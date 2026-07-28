import { describe, expect, it } from "vitest";
import {
  applyCoverTheme,
  buildCoverTheme,
  clearCoverTheme,
  extractDominantColor,
} from "./dynamic-theme";

function pixels(colors: ReadonlyArray<readonly [number, number, number, number]>) {
  return new Uint8ClampedArray(colors.flat());
}

function colorDistance(first: string, second: string) {
  const firstChannels = first.match(/\d+/g)?.map(Number) ?? [];
  const secondChannels = second.match(/\d+/g)?.map(Number) ?? [];
  return Math.hypot(
    (firstChannels[0] ?? 0) - (secondChannels[0] ?? 0),
    (firstChannels[1] ?? 0) - (secondChannels[1] ?? 0),
    (firstChannels[2] ?? 0) - (secondChannels[2] ?? 0),
  );
}

describe("dynamic cover themes", () => {
  it("finds the strongest visible color group and ignores transparent pixels", () => {
    const dominant = extractDominantColor(pixels([
      [245, 245, 245, 255],
      [220, 42, 56, 255],
      [216, 46, 60, 255],
      [222, 40, 54, 255],
      [24, 80, 210, 255],
      [20, 200, 40, 0],
    ]));

    expect(dominant).toEqual({ red: 219, green: 43, blue: 57 });
  });

  it("falls back to a neutral cover color when no chromatic pixels exist", () => {
    expect(extractDominantColor(pixels([
      [248, 248, 248, 255],
      [249, 249, 249, 255],
    ]))).toEqual({ red: 249, green: 249, blue: 249 });
  });

  it("derives distinct DaisyUI brand colors with readable content colors", () => {
    const theme = buildCoverTheme({ red: 24, green: 92, blue: 178 });

    expect(theme.primary).toMatch(/^rgb\(\d+ \d+ \d+\)$/);
    expect(new Set([theme.primary, theme.secondary, theme.accent]).size).toBe(3);
    expect(["rgb(0 0 0)", "rgb(255 255 255)"]).toContain(theme.primaryContent);
    expect(["rgb(0 0 0)", "rgb(255 255 255)"]).toContain(theme.secondaryContent);
    expect(["rgb(0 0 0)", "rgb(255 255 255)"]).toContain(theme.accentContent);
  });

  it("changes the common DaisyUI surfaces when the cover color changes", () => {
    const warmTheme = buildCoverTheme({ red: 188, green: 48, blue: 38 });
    const coolTheme = buildCoverTheme({ red: 24, green: 92, blue: 178 });

    expect(warmTheme.base100).not.toBe(coolTheme.base100);
    expect(warmTheme.base200).not.toBe(coolTheme.base200);
    expect(warmTheme.base300).not.toBe(coolTheme.base300);
    expect(colorDistance(warmTheme.base200, coolTheme.base200)).toBeGreaterThan(24);
  });

  it("applies and clears all dynamic DaisyUI color variables", () => {
    const theme = buildCoverTheme({ red: 180, green: 48, blue: 92 });
    const element = document.createElement("div");

    applyCoverTheme(element, theme);
    expect(element.style.getPropertyValue("--color-base-100")).toBe(theme.base100);
    expect(element.style.getPropertyValue("--color-base-200")).toBe(theme.base200);
    expect(element.style.getPropertyValue("--color-primary")).toBe(theme.primary);
    expect(element.style.getPropertyValue("--color-secondary")).toBe(theme.secondary);
    expect(element.style.getPropertyValue("--color-accent-content")).toBe(theme.accentContent);

    clearCoverTheme(element);
    expect(element.style.getPropertyValue("--color-base-100")).toBe("");
    expect(element.style.getPropertyValue("--color-base-200")).toBe("");
    expect(element.style.getPropertyValue("--color-primary")).toBe("");
    expect(element.style.getPropertyValue("--color-secondary")).toBe("");
    expect(element.style.getPropertyValue("--color-accent-content")).toBe("");
  });
});
