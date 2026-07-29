import { afterEach, describe, expect, it } from "vitest";
import {
  currentLocale,
  formatNumber,
  isSupportedLocale,
  setLocale,
  t,
} from ".";

describe("i18n", () => {
  afterEach(() => setLocale("en"));

  it("uses English source messages as the default locale and fallback", () => {
    expect(t("Settings")).toBe("Settings");
    setLocale("zh-CN");
    expect(t("An untranslated message")).toBe("An untranslated message");
  });

  it("translates Simplified Chinese messages and interpolates values", () => {
    setLocale("zh-CN");

    expect(currentLocale.value).toBe("zh-CN");
    expect(t("Simplified Chinese")).toBe("简体中文");
    expect(t("{count} tracks", { count: 12 })).toBe("12 首歌曲");
  });

  it("validates supported locales and updates the document language", () => {
    expect(isSupportedLocale("en")).toBe(true);
    expect(isSupportedLocale("zh-CN")).toBe(true);
    expect(isSupportedLocale("zh-TW")).toBe(false);

    setLocale("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
  });

  it("formats numbers using the active locale", () => {
    setLocale("en");
    expect(formatNumber(12_345)).toBe("12,345");
    setLocale("zh-CN");
    expect(formatNumber(12_345)).toBe("12,345");
  });
});
