import { afterEach, describe, expect, it, vi } from "vitest";
import { viewportMenuPosition } from "./viewport-layout";

describe("viewport layout", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("style");
    vi.unstubAllGlobals();
  });

  it("keeps floating menus inside system bars and display cutouts", () => {
    document.documentElement.style.setProperty("--safe-area-top", "24px");
    document.documentElement.style.setProperty("--safe-area-right", "6px");
    document.documentElement.style.setProperty("--safe-area-bottom", "16px");
    document.documentElement.style.setProperty("--safe-area-left", "4px");
    vi.stubGlobal("innerWidth", 360);
    vi.stubGlobal("innerHeight", 800);

    expect(viewportMenuPosition(0, 0, 224, 80)).toEqual({ x: 12, y: 32 });
    expect(viewportMenuPosition(350, 790, 224, 80)).toEqual({ x: 122, y: 696 });
  });

  it("accounts for the visible viewport when the keyboard or zoom changes it", () => {
    vi.stubGlobal("visualViewport", {
      height: 420,
      offsetLeft: 10,
      offsetTop: 120,
      width: 300,
    });

    expect(viewportMenuPosition(0, 0, 240, 180)).toEqual({ x: 18, y: 128 });
    expect(viewportMenuPosition(400, 600, 240, 180)).toEqual({ x: 62, y: 352 });
  });
});
