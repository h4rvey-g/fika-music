import { isAudioSourceId, type AudioSourceId } from "./audio-source-api";

export type ThemeCategory = "bright" | "dark";

export const THEME_OPTIONS = [
  { value: "system", label: "System", category: null },
  { value: "dynamic", label: "Dynamic (cover art)", category: null },
  { value: "light", label: "Light", category: "bright" },
  { value: "dark", label: "Dark", category: "dark" },
  { value: "cupcake", label: "Cupcake", category: "bright" },
  { value: "bumblebee", label: "Bumblebee", category: "bright" },
  { value: "emerald", label: "Emerald", category: "bright" },
  { value: "corporate", label: "Corporate", category: "bright" },
  { value: "synthwave", label: "Synthwave", category: "dark" },
  { value: "retro", label: "Retro", category: "bright" },
  { value: "cyberpunk", label: "Cyberpunk", category: "bright" },
  { value: "valentine", label: "Valentine", category: "bright" },
  { value: "halloween", label: "Halloween", category: "dark" },
  { value: "garden", label: "Garden", category: "bright" },
  { value: "forest", label: "Forest", category: "dark" },
  { value: "aqua", label: "Aqua", category: "dark" },
  { value: "lofi", label: "Lo-Fi", category: "bright" },
  { value: "pastel", label: "Pastel", category: "bright" },
  { value: "fantasy", label: "Fantasy", category: "bright" },
  { value: "wireframe", label: "Wireframe", category: "bright" },
  { value: "black", label: "Black", category: "dark" },
  { value: "luxury", label: "Luxury", category: "dark" },
  { value: "dracula", label: "Dracula", category: "dark" },
  { value: "cmyk", label: "CMYK", category: "bright" },
  { value: "autumn", label: "Autumn", category: "bright" },
  { value: "business", label: "Business", category: "dark" },
  { value: "acid", label: "Acid", category: "bright" },
  { value: "lemonade", label: "Lemonade", category: "bright" },
  { value: "night", label: "Night", category: "dark" },
  { value: "coffee", label: "Coffee", category: "dark" },
  { value: "winter", label: "Winter", category: "bright" },
  { value: "dim", label: "Dim", category: "dark" },
  { value: "nord", label: "Nord", category: "bright" },
  { value: "sunset", label: "Sunset", category: "dark" },
  { value: "caramellatte", label: "Caramellatte", category: "bright" },
  { value: "abyss", label: "Abyss", category: "dark" },
  { value: "silk", label: "Silk", category: "bright" },
] as const;

export type ThemePreference = (typeof THEME_OPTIONS)[number]["value"];

export const THEME_MODE_OPTIONS = THEME_OPTIONS.filter((option) => option.category === null);

export const THEME_GROUPS: ReadonlyArray<{
  value: ThemeCategory;
  label: string;
  options: ReadonlyArray<(typeof THEME_OPTIONS)[number]>;
}> = [
  {
    value: "bright",
    label: "Bright",
    options: THEME_OPTIONS.filter((option) => option.category === "bright"),
  },
  {
    value: "dark",
    label: "Dark",
    options: THEME_OPTIONS.filter((option) => option.category === "dark"),
  },
];

export type LayoutDensity = "comfortable" | "compact";
export type StreamQuality = "128k" | "320k" | "flac" | "flac24bit";
export type PlaybackMode = "sequential" | "shuffle" | "repeat";

export type UiPreferences = {
  theme: ThemePreference;
  density: LayoutDensity;
  streamQuality: StreamQuality;
  audioSourceId: AudioSourceId;
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
  audioSourceId: "",
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
  const candidate =
    value && typeof value === "object"
      ? (value as Partial<UiPreferences> & { audioSourceFamily?: unknown })
      : {};
  const storedAudioSourceId = candidate.audioSourceId ?? candidate.audioSourceFamily;

  return {
    theme: isThemePreference(candidate.theme) ? candidate.theme : DEFAULT_UI_PREFERENCES.theme,
    density: isLayoutDensity(candidate.density) ? candidate.density : DEFAULT_UI_PREFERENCES.density,
    streamQuality: isStreamQuality(candidate.streamQuality)
      ? candidate.streamQuality
      : DEFAULT_UI_PREFERENCES.streamQuality,
    audioSourceId: isAudioSourceId(storedAudioSourceId)
      ? storedAudioSourceId
      : DEFAULT_UI_PREFERENCES.audioSourceId,
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
