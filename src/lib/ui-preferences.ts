export type ThemePreference = "system" | "light" | "dark";
export type LayoutDensity = "comfortable" | "compact";
export type StreamQuality = "128k" | "320k" | "flac" | "flac24bit";

export type UiPreferences = {
  theme: ThemePreference;
  density: LayoutDensity;
  streamQuality: StreamQuality;
  volume: number;
};

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export const UI_PREFERENCES_STORAGE_KEY = "fika.ui-preferences";

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  theme: "system",
  density: "comfortable",
  streamQuality: "128k",
  volume: 0.8,
};

export function loadUiPreferences(storage: ReadableStorage | null = browserStorage()): UiPreferences {
  if (!storage) {
    return { ...DEFAULT_UI_PREFERENCES };
  }

  try {
    const storedValue = storage.getItem(UI_PREFERENCES_STORAGE_KEY);
    return storedValue ? parseUiPreferences(JSON.parse(storedValue)) : { ...DEFAULT_UI_PREFERENCES };
  } catch {
    return { ...DEFAULT_UI_PREFERENCES };
  }
}

export function saveUiPreferences(
  preferences: UiPreferences,
  storage: WritableStorage | null = browserStorage(),
) {
  if (!storage) {
    return;
  }

  try {
    storage.setItem(UI_PREFERENCES_STORAGE_KEY, JSON.stringify(parseUiPreferences(preferences)));
  } catch {
    // Preferences should never prevent the application shell from rendering.
  }
}

export function parseUiPreferences(value: unknown): UiPreferences {
  const candidate = value && typeof value === "object" ? (value as Partial<UiPreferences>) : {};

  return {
    theme: isThemePreference(candidate.theme) ? candidate.theme : DEFAULT_UI_PREFERENCES.theme,
    density: isLayoutDensity(candidate.density) ? candidate.density : DEFAULT_UI_PREFERENCES.density,
    streamQuality: isStreamQuality(candidate.streamQuality)
      ? candidate.streamQuality
      : DEFAULT_UI_PREFERENCES.streamQuality,
    volume:
      typeof candidate.volume === "number" && Number.isFinite(candidate.volume)
        ? Math.min(1, Math.max(0, candidate.volume))
        : DEFAULT_UI_PREFERENCES.volume,
  };
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function isLayoutDensity(value: unknown): value is LayoutDensity {
  return value === "comfortable" || value === "compact";
}

function isStreamQuality(value: unknown): value is StreamQuality {
  return value === "128k" || value === "320k" || value === "flac" || value === "flac24bit";
}
