import { isAudioSourceFamily, type AudioSourceFamily } from "./audio-source-api";

export const THEME_OPTIONS = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "cupcake", label: "Cupcake" },
  { value: "bumblebee", label: "Bumblebee" },
  { value: "emerald", label: "Emerald" },
  { value: "corporate", label: "Corporate" },
  { value: "synthwave", label: "Synthwave" },
  { value: "retro", label: "Retro" },
  { value: "cyberpunk", label: "Cyberpunk" },
  { value: "valentine", label: "Valentine" },
  { value: "halloween", label: "Halloween" },
  { value: "garden", label: "Garden" },
  { value: "forest", label: "Forest" },
  { value: "aqua", label: "Aqua" },
  { value: "lofi", label: "Lo-Fi" },
  { value: "pastel", label: "Pastel" },
  { value: "fantasy", label: "Fantasy" },
  { value: "wireframe", label: "Wireframe" },
  { value: "black", label: "Black" },
  { value: "luxury", label: "Luxury" },
  { value: "dracula", label: "Dracula" },
  { value: "cmyk", label: "CMYK" },
  { value: "autumn", label: "Autumn" },
  { value: "business", label: "Business" },
  { value: "acid", label: "Acid" },
  { value: "lemonade", label: "Lemonade" },
  { value: "night", label: "Night" },
  { value: "coffee", label: "Coffee" },
  { value: "winter", label: "Winter" },
  { value: "dim", label: "Dim" },
  { value: "nord", label: "Nord" },
  { value: "sunset", label: "Sunset" },
  { value: "caramellatte", label: "Caramellatte" },
  { value: "abyss", label: "Abyss" },
  { value: "silk", label: "Silk" },
] as const;

export type ThemePreference = (typeof THEME_OPTIONS)[number]["value"];
export type LayoutDensity = "comfortable" | "compact";
export type StreamQuality = "128k" | "320k" | "flac" | "flac24bit";
export type PlaybackMode = "sequential" | "shuffle" | "repeat";

export type UiPreferences = {
  theme: ThemePreference;
  density: LayoutDensity;
  streamQuality: StreamQuality;
  audioSourceFamily: AudioSourceFamily;
  volume: number;
  playbackMode: PlaybackMode;
};

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export const UI_PREFERENCES_STORAGE_KEY = "fika.ui-preferences";

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  theme: "system",
  density: "comfortable",
  streamQuality: "128k",
  audioSourceFamily: "nianxin",
  volume: 0.8,
  playbackMode: "sequential",
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
    audioSourceFamily: isAudioSourceFamily(candidate.audioSourceFamily)
      ? candidate.audioSourceFamily
      : DEFAULT_UI_PREFERENCES.audioSourceFamily,
    volume:
      typeof candidate.volume === "number" && Number.isFinite(candidate.volume)
        ? Math.min(1, Math.max(0, candidate.volume))
        : DEFAULT_UI_PREFERENCES.volume,
    playbackMode: isPlaybackMode(candidate.playbackMode)
      ? candidate.playbackMode
      : DEFAULT_UI_PREFERENCES.playbackMode,
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
  return THEME_OPTIONS.some((option) => option.value === value);
}

function isLayoutDensity(value: unknown): value is LayoutDensity {
  return value === "comfortable" || value === "compact";
}

function isStreamQuality(value: unknown): value is StreamQuality {
  return value === "128k" || value === "320k" || value === "flac" || value === "flac24bit";
}

function isPlaybackMode(value: unknown): value is PlaybackMode {
  return value === "sequential" || value === "shuffle" || value === "repeat";
}
