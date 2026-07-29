import { readonly, ref, watch, type Ref } from "vue";
import { ZH_CN_MESSAGES } from "./zh-CN";

export const SUPPORTED_LOCALES = ["en", "zh-CN"] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];
export type TranslationValues = Readonly<Record<string, string | number>>;

export const LOCALE_OPTIONS: ReadonlyArray<{
  value: SupportedLocale;
  label: string;
}> = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "简体中文" },
];

const activeLocale = ref<SupportedLocale>("en");

export const currentLocale: Readonly<Ref<SupportedLocale>> = readonly(activeLocale);

watch(
  activeLocale,
  (locale) => {
    if (typeof document !== "undefined") {
      document.documentElement.lang = locale;
    }
  },
  { immediate: true, flush: "sync" },
);

export function isSupportedLocale(value: unknown): value is SupportedLocale {
  return SUPPORTED_LOCALES.some((locale) => locale === value);
}

export function setLocale(locale: SupportedLocale) {
  activeLocale.value = locale;
}

export function t(message: string, values: TranslationValues = {}): string {
  const template = activeLocale.value === "zh-CN"
    ? ZH_CN_MESSAGES[message] ?? message
    : message;

  return template.replace(/\{([A-Za-z][A-Za-z0-9_]*)\}/g, (token, key: string) => {
    const value = values[key];
    return value === undefined ? token : String(value);
  });
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(activeLocale.value).format(value);
}
