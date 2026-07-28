export const NOW_PLAYING_LYRICS_STORAGE_KEY = "fika.now-playing-lyrics";
export const NOW_PLAYING_LYRICS_SETTINGS_ID = "now-playing-lyrics-settings";
export const NOW_PLAYING_LYRICS_THEME_COLOR = "theme";

export const NOW_PLAYING_LYRICS_FONT_OPTIONS = [
  { value: "system", label: "System" },
  { value: "sans", label: "Sans serif" },
  { value: "serif", label: "Serif" },
  { value: "rounded", label: "Rounded" },
  { value: "monospace", label: "Monospace" },
] as const;

export type NowPlayingLyricsFont =
  (typeof NOW_PLAYING_LYRICS_FONT_OPTIONS)[number]["value"];
export type NowPlayingLyricsAlignment = "left" | "center" | "right";
export type NowPlayingLyricsFontWeight = 400 | 500 | 600 | 700 | 800;

export type NowPlayingLyricsPreferences = {
  font: NowPlayingLyricsFont;
  fontSize: number;
  lineGap: number;
  activeFontWeight: NowPlayingLyricsFontWeight;
  alignment: NowPlayingLyricsAlignment;
  activeColor: string;
  inactiveColor: string;
  inactiveOpacity: number;
};

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/i;

export const DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES: NowPlayingLyricsPreferences = {
  font: "system",
  fontSize: 14,
  lineGap: 16,
  activeFontWeight: 600,
  alignment: "center",
  activeColor: NOW_PLAYING_LYRICS_THEME_COLOR,
  inactiveColor: NOW_PLAYING_LYRICS_THEME_COLOR,
  inactiveOpacity: 0.45,
};

export function loadNowPlayingLyricsPreferences(
  storage: ReadableStorage | null = browserStorage(),
): NowPlayingLyricsPreferences {
  if (!storage) return { ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES };

  try {
    const value = storage.getItem(NOW_PLAYING_LYRICS_STORAGE_KEY);
    return value
      ? parseNowPlayingLyricsPreferences(JSON.parse(value))
      : { ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES };
  } catch {
    return { ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES };
  }
}

export function saveNowPlayingLyricsPreferences(
  preferences: NowPlayingLyricsPreferences,
  storage: WritableStorage | null = browserStorage(),
) {
  if (!storage) return;

  try {
    storage.setItem(
      NOW_PLAYING_LYRICS_STORAGE_KEY,
      JSON.stringify(parseNowPlayingLyricsPreferences(preferences)),
    );
  } catch {
    // Lyrics preferences should not prevent the application shell from rendering.
  }
}

export function parseNowPlayingLyricsPreferences(
  value: unknown,
): NowPlayingLyricsPreferences {
  const candidate = value && typeof value === "object"
    ? (value as Partial<NowPlayingLyricsPreferences>)
    : {};

  return {
    font: isFont(candidate.font)
      ? candidate.font
      : DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.font,
    fontSize: clampNumber(
      candidate.fontSize,
      12,
      30,
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.fontSize,
    ),
    lineGap: clampNumber(
      candidate.lineGap,
      4,
      28,
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.lineGap,
    ),
    activeFontWeight: isFontWeight(candidate.activeFontWeight)
      ? candidate.activeFontWeight
      : DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.activeFontWeight,
    alignment: isAlignment(candidate.alignment)
      ? candidate.alignment
      : DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.alignment,
    activeColor: colorValue(
      candidate.activeColor,
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.activeColor,
    ),
    inactiveColor: colorValue(
      candidate.inactiveColor,
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.inactiveColor,
    ),
    inactiveOpacity: clampNumber(
      candidate.inactiveOpacity,
      0.1,
      1,
      DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES.inactiveOpacity,
    ),
  };
}

export function nowPlayingLyricsFontFamily(font: NowPlayingLyricsFont) {
  switch (font) {
    case "sans":
      return "Arial, Helvetica, sans-serif";
    case "serif":
      return "Georgia, 'Times New Roman', serif";
    case "rounded":
      return "ui-rounded, 'SF Pro Rounded', 'Segoe UI Rounded', sans-serif";
    case "monospace":
      return "ui-monospace, 'SFMono-Regular', Consolas, monospace";
    default:
      return "inherit";
  }
}

export function nowPlayingLyricsColor(color: string) {
  return color === NOW_PLAYING_LYRICS_THEME_COLOR
    ? "var(--color-base-content)"
    : color;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function isFont(value: unknown): value is NowPlayingLyricsFont {
  return NOW_PLAYING_LYRICS_FONT_OPTIONS.some((option) => option.value === value);
}

function isFontWeight(value: unknown): value is NowPlayingLyricsFontWeight {
  return value === 400 || value === 500 || value === 600 || value === 700 || value === 800;
}

function isAlignment(value: unknown): value is NowPlayingLyricsAlignment {
  return value === "left" || value === "center" || value === "right";
}

function colorValue(value: unknown, fallback: string) {
  if (value === NOW_PLAYING_LYRICS_THEME_COLOR) {
    return NOW_PLAYING_LYRICS_THEME_COLOR;
  }
  return typeof value === "string" && HEX_COLOR_PATTERN.test(value) ? value : fallback;
}

function clampNumber(value: unknown, min: number, max: number, fallback: number) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}
