import { describe, expect, it } from "vitest";
import androidConfig from "../src-tauri/tauri.android.conf.json";

const androidIconDensities = ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"];
const androidIconFiles = [
  "mipmap-anydpi-v26/ic_launcher.xml",
  "values/ic_launcher_background.xml",
  ...androidIconDensities.flatMap((density) => [
    `mipmap-${density}/ic_launcher.png`,
    `mipmap-${density}/ic_launcher_round.png`,
    `mipmap-${density}/ic_launcher_foreground.png`,
  ]),
].sort();
const includedAndroidIconFiles = Object.keys(
  import.meta.glob("../src-tauri/icons/android/**/*", {
    eager: true,
    import: "default",
    query: "?url",
  }),
)
  .map((file) => file.replace("../src-tauri/icons/android/", ""))
  .sort();

describe("Android Tauri configuration", () => {
  it("starts only the main application webview", () => {
    const windows = (androidConfig as {
      app?: { windows?: Array<{ label?: string; url?: string }> };
    }).app?.windows;

    expect(windows).toEqual([
      {
        label: "main",
        title: "Fika Music",
      },
    ]);
  });

  it("provides launcher icons for every Android density", () => {
    expect(includedAndroidIconFiles).toEqual(androidIconFiles);
  });
});
