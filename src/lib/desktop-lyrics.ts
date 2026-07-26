import type { LyricLine, LyricWord, ResolvedLyrics } from "../generated/bindings";

export const DESKTOP_LYRICS_WINDOW_LABEL = "desktop-lyrics";
export const DESKTOP_LYRICS_STATE_EVENT = "desktop-lyrics:state";
export const DESKTOP_LYRICS_READY_EVENT = "desktop-lyrics:ready";
export const DESKTOP_LYRICS_HIDE_EVENT = "desktop-lyrics:hide";
export const DESKTOP_LYRICS_LOCK_EVENT = "desktop-lyrics:set-lock";
export const DESKTOP_LYRICS_UPDATE_EVENT = "desktop-lyrics:update-preferences";
export const DESKTOP_LYRICS_STORAGE_KEY = "fika.desktop-lyrics";
export const DESKTOP_LYRICS_TRANSPARENT_COLOR = "transparent";

export const DESKTOP_LYRICS_FONT_OPTIONS = [
  { value: "system", label: "System" },
  { value: "sans", label: "Sans serif" },
  { value: "serif", label: "Serif" },
  { value: "rounded", label: "Rounded" },
  { value: "monospace", label: "Monospace" },
] as const;

export type DesktopLyricsFont = (typeof DESKTOP_LYRICS_FONT_OPTIONS)[number]["value"];
export type DesktopLyricsAlignment = "left" | "center" | "right";
export type DesktopLyricsEffect = "shadow" | "outline" | "none";
export type DesktopLyricsTimingSource = "source" | "estimated" | null;

export type DesktopLyricWordTiming = {
  text: string;
  startMs: number;
  endMs: number;
  isTimed?: boolean;
};

export type DesktopLyricLines = {
  currentLine: string;
  currentLineKey: string;
  currentLineStartMs: number | null;
  currentLineEndMs: number | null;
  currentWords: DesktopLyricWordTiming[];
  currentTimingSource: DesktopLyricsTimingSource;
  nextLine: string | null;
};

export type DesktopLyricsPreferences = {
  enabled: boolean;
  menuBarEnabled: boolean;
  menuBarMaxWidth: number;
  locked: boolean;
  alwaysOnTop: boolean;
  showNextLine: boolean;
  activeColor: string;
  inactiveColor: string;
  backgroundColor: string;
  backgroundOpacity: number;
  fontSize: number;
  fontWeight: number;
  font: DesktopLyricsFont;
  alignment: DesktopLyricsAlignment;
  effect: DesktopLyricsEffect;
};

export type DesktopLyricsState = {
  title: string;
  subtitle: string;
  currentLine: string;
  currentLineKey: string;
  currentLineStartMs: number | null;
  currentLineEndMs: number | null;
  currentWords: DesktopLyricWordTiming[];
  currentTimingSource: DesktopLyricsTimingSource;
  nextLine: string | null;
  isPlaying: boolean;
  clockRunning: boolean;
  playbackRate: number;
  playbackPositionMs: number;
  preferences: DesktopLyricsPreferences;
};

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/i;

export const DEFAULT_DESKTOP_LYRICS_PREFERENCES: DesktopLyricsPreferences = {
  enabled: false,
  menuBarEnabled: false,
  menuBarMaxWidth: 40,
  locked: false,
  alwaysOnTop: true,
  showNextLine: true,
  activeColor: "#7dd3fc",
  inactiveColor: "#f8fafc",
  backgroundColor: DESKTOP_LYRICS_TRANSPARENT_COLOR,
  backgroundOpacity: 0.58,
  fontSize: 34,
  fontWeight: 700,
  font: "system",
  alignment: "center",
  effect: "shadow",
};

export function loadDesktopLyricsPreferences(
  storage: ReadableStorage | null = browserStorage(),
): DesktopLyricsPreferences {
  if (!storage) return { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES };

  try {
    const value = storage.getItem(DESKTOP_LYRICS_STORAGE_KEY);
    return value
      ? parseDesktopLyricsPreferences(JSON.parse(value))
      : { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES };
  } catch {
    return { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES };
  }
}

export function saveDesktopLyricsPreferences(
  preferences: DesktopLyricsPreferences,
  storage: WritableStorage | null = browserStorage(),
) {
  if (!storage) return;

  try {
    storage.setItem(
      DESKTOP_LYRICS_STORAGE_KEY,
      JSON.stringify(parseDesktopLyricsPreferences(preferences)),
    );
  } catch {
    // Desktop lyrics preferences should not prevent playback from continuing.
  }
}

export function parseDesktopLyricsPreferences(value: unknown): DesktopLyricsPreferences {
  const candidate = value && typeof value === "object"
    ? (value as Partial<DesktopLyricsPreferences>)
    : {};

  return {
    enabled: booleanValue(candidate.enabled, DEFAULT_DESKTOP_LYRICS_PREFERENCES.enabled),
    menuBarEnabled: booleanValue(
      candidate.menuBarEnabled,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.menuBarEnabled,
    ),
    menuBarMaxWidth: clampNumber(
      candidate.menuBarMaxWidth,
      24,
      56,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.menuBarMaxWidth,
    ),
    locked: booleanValue(candidate.locked, DEFAULT_DESKTOP_LYRICS_PREFERENCES.locked),
    alwaysOnTop: booleanValue(
      candidate.alwaysOnTop,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.alwaysOnTop,
    ),
    showNextLine: booleanValue(
      candidate.showNextLine,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.showNextLine,
    ),
    activeColor: colorValue(
      candidate.activeColor,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.activeColor,
    ),
    inactiveColor: colorValue(
      candidate.inactiveColor,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.inactiveColor,
    ),
    backgroundColor: colorValue(
      candidate.backgroundColor,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.backgroundColor,
    ),
    backgroundOpacity: clampNumber(
      candidate.backgroundOpacity,
      0,
      1,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.backgroundOpacity,
    ),
    fontSize: clampNumber(
      candidate.fontSize,
      18,
      72,
      DEFAULT_DESKTOP_LYRICS_PREFERENCES.fontSize,
    ),
    fontWeight: isDesktopLyricsFontWeight(candidate.fontWeight)
      ? candidate.fontWeight
      : DEFAULT_DESKTOP_LYRICS_PREFERENCES.fontWeight,
    font: isDesktopLyricsFont(candidate.font)
      ? candidate.font
      : DEFAULT_DESKTOP_LYRICS_PREFERENCES.font,
    alignment: isDesktopLyricsAlignment(candidate.alignment)
      ? candidate.alignment
      : DEFAULT_DESKTOP_LYRICS_PREFERENCES.alignment,
    effect: isDesktopLyricsEffect(candidate.effect)
      ? candidate.effect
      : DEFAULT_DESKTOP_LYRICS_PREFERENCES.effect,
  };
}

export function resolveDesktopLyricLines(
  lyrics: ResolvedLyrics | null,
  playbackPosition: number,
  playbackDuration = 0,
): DesktopLyricLines {
  const lines = lyrics?.lines ?? [];
  if (!lines.length) {
    return desktopLyricsMessage("No lyrics available");
  }

  if (!lyrics?.isSynced) {
    const durationMs = secondsToMilliseconds(playbackDuration);
    if (durationMs > 0) {
      return resolveEstimatedPlainLyrics(
        lines,
        secondsToMilliseconds(playbackPosition),
        durationMs,
      );
    }
    return {
      currentLine: lines[0]?.text || "No lyrics available",
      currentLineKey: "plain:0",
      currentLineStartMs: null,
      currentLineEndMs: null,
      currentWords: [],
      currentTimingSource: null,
      nextLine: lines[1]?.text || null,
    };
  }

  const positionMs = secondsToMilliseconds(playbackPosition);
  let activeIndex = -1;
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const startMs = lines[index].startMs;
    if (startMs !== null && startMs <= positionMs) {
      activeIndex = index;
      break;
    }
  }

  if (activeIndex < 0) {
    activeIndex = 0;
  }

  return resolvedTimedLine(
    lines,
    activeIndex,
    secondsToMilliseconds(playbackDuration),
  );
}

export function desktopLyricsMessage(message: string): DesktopLyricLines {
  return {
    currentLine: message,
    currentLineKey: `message:${message}`,
    currentLineStartMs: null,
    currentLineEndMs: null,
    currentWords: [],
    currentTimingSource: null,
    nextLine: null,
  };
}

export function desktopLyricsMinimumHeight(preferences: DesktopLyricsPreferences) {
  const toolbarHeight = 36;
  const verticalPadding = 16;
  const currentLineHeight = preferences.fontSize * 1.12 * 2;
  const nextLineHeight = preferences.showNextLine
    ? Math.max(14, Math.round(preferences.fontSize * 0.52)) * 1.2 + 8
    : 0;
  return Math.ceil(toolbarHeight + verticalPadding + currentLineHeight + nextLineHeight);
}

export function desktopLyricsOutlineColor(color: string) {
  if (color === DESKTOP_LYRICS_TRANSPARENT_COLOR) {
    return DESKTOP_LYRICS_TRANSPARENT_COLOR;
  }
  const red = Number.parseInt(color.slice(1, 3), 16) / 255;
  const green = Number.parseInt(color.slice(3, 5), 16) / 255;
  const blue = Number.parseInt(color.slice(5, 7), 16) / 255;
  const linear = [red, green, blue].map((channel) => (
    channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4
  ));
  const luminance = (linear[0] ?? 0) * 0.2126
    + (linear[1] ?? 0) * 0.7152
    + (linear[2] ?? 0) * 0.0722;
  return luminance < 0.24
    ? "rgb(255 255 255 / 88%)"
    : "rgb(0 0 0 / 82%)";
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function booleanValue(value: unknown, fallback: boolean) {
  return typeof value === "boolean" ? value : fallback;
}

function colorValue(value: unknown, fallback: string) {
  if (
    typeof value === "string"
    && value.toLowerCase() === DESKTOP_LYRICS_TRANSPARENT_COLOR
  ) {
    return DESKTOP_LYRICS_TRANSPARENT_COLOR;
  }
  return typeof value === "string" && HEX_COLOR_PATTERN.test(value) ? value : fallback;
}

function clampNumber(value: unknown, min: number, max: number, fallback: number) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}

function resolvedTimedLine(
  lines: LyricLine[],
  activeIndex: number,
  playbackDurationMs: number,
): DesktopLyricLines {
  const line = lines[activeIndex];
  if (!line) return desktopLyricsMessage("No lyrics available");

  const startMs = line.startMs ?? 0;
  const nextStartMs = lines[activeIndex + 1]?.startMs ?? null;
  const endMs = effectiveLineEnd(line, startMs, nextStartMs, playbackDurationMs);
  const lineWords = line.words ?? [];
  const sourceWords = validSourceWords(lineWords, line.text)
    ? expandSourceWords(lineWords, line.text, endMs)
    : [];
  const currentWords = sourceWords.length
    ? sourceWords
    : estimateWordTimings(line.text, startMs, endMs);

  return {
    currentLine: line.text || "No lyrics available",
    currentLineKey: `timed:${activeIndex}:${startMs}`,
    currentLineStartMs: startMs,
    currentLineEndMs: endMs,
    currentWords,
    currentTimingSource: sourceWords.length ? "source" : "estimated",
    nextLine: lines[activeIndex + 1]?.text || null,
  };
}

function resolveEstimatedPlainLyrics(
  lines: LyricLine[],
  positionMs: number,
  durationMs: number,
): DesktopLyricLines {
  const weights = lines.map((line) => Math.max(1, lyricTextWeight(line.text)));
  const totalWeight = weights.reduce((total, weight) => total + weight, 0);
  let elapsedWeight = 0;
  let activeIndex = lines.length - 1;
  let startMs = 0;
  let endMs = durationMs;

  for (let index = 0; index < lines.length; index += 1) {
    const candidateStart = Math.round(durationMs * elapsedWeight / totalWeight);
    elapsedWeight += weights[index] ?? 0;
    const candidateEnd = index === lines.length - 1
      ? durationMs
      : Math.round(durationMs * elapsedWeight / totalWeight);
    if (positionMs < candidateEnd || index === lines.length - 1) {
      activeIndex = index;
      startMs = candidateStart;
      endMs = candidateEnd;
      break;
    }
  }

  const line = lines[activeIndex];
  if (!line) return desktopLyricsMessage("No lyrics available");
  return {
    currentLine: line.text || "No lyrics available",
    currentLineKey: `plain-estimated:${activeIndex}`,
    currentLineStartMs: startMs,
    currentLineEndMs: endMs,
    currentWords: estimateWordTimings(line.text, startMs, endMs),
    currentTimingSource: "estimated",
    nextLine: lines[activeIndex + 1]?.text || null,
  };
}

function effectiveLineEnd(
  line: LyricLine,
  startMs: number,
  nextStartMs: number | null,
  playbackDurationMs: number,
) {
  const candidates = [line.endMs, nextStartMs, playbackDurationMs]
    .filter((value): value is number => value !== null && value > startMs);
  if (candidates.length) return Math.min(...candidates);
  return startMs + Math.min(12_000, Math.max(2_000, lyricTextWeight(line.text) * 280));
}

function validSourceWords(words: LyricWord[], lineText: string) {
  const timedText = words.map((word) => word.text).join("");
  return words.length > 0
    && words.every((word) => Number.isFinite(word.startMs) && Number.isFinite(word.endMs))
    && timedTextOffset(lineText, timedText) !== null;
}

function expandSourceWords(words: LyricWord[], lineText: string, lineEndMs: number) {
  const timedText = words.map((word) => word.text).join("");
  const offset = timedTextOffset(lineText, timedText);
  if (offset === null) return [];

  const expandedSource = words.flatMap((word) => estimateWordTimings(
    word.text,
    Math.max(0, word.startMs),
    Math.max(word.startMs, word.endMs),
  ));
  const sourceStartMs = Math.min(...words.map((word) => Math.max(0, word.startMs)));
  const sourceEndMs = Math.max(...words.map((word) => Math.max(word.startMs, word.endMs)));
  const companionEndMs = sourceEndMs > sourceStartMs
    ? sourceEndMs
    : Math.max(sourceStartMs, lineEndMs);

  return [
    ...estimateWordTimings(lineText.slice(0, offset), sourceStartMs, companionEndMs),
    ...expandedSource,
    ...estimateWordTimings(
      lineText.slice(offset + timedText.length),
      sourceStartMs,
      companionEndMs,
    ),
  ];
}

function timedTextOffset(lineText: string, timedText: string) {
  if (!timedText) return null;
  let offset = lineText.indexOf(timedText);
  while (offset >= 0) {
    const end = offset + timedText.length;
    const startsAtLineBoundary = offset === 0 || lineText[offset - 1] === "\n";
    const endsAtLineBoundary = end === lineText.length || lineText[end] === "\n";
    if (startsAtLineBoundary && endsAtLineBoundary) return offset;
    offset = lineText.indexOf(timedText, offset + 1);
  }
  return null;
}

function estimateWordTimings(text: string, startMs: number, endMs: number) {
  const characters = splitGraphemes(text);
  if (!characters.length) return [];
  const weights = characters.map(characterWeight);
  const totalWeight = weights.reduce((total, weight) => total + weight, 0);
  const durationMs = Math.max(0, endMs - startMs);
  let elapsedWeight = 0;

  return characters.map((text, index) => {
    const characterStartMs = Math.round(startMs + durationMs * elapsedWeight / totalWeight);
    elapsedWeight += weights[index] ?? 0;
    const characterEndMs = index === characters.length - 1
      ? endMs
      : Math.round(startMs + durationMs * elapsedWeight / totalWeight);
    return { text, startMs: characterStartMs, endMs: characterEndMs };
  });
}

function lyricTextWeight(text: string) {
  return splitGraphemes(text).reduce(
    (total, character) => total + characterWeight(character),
    0,
  );
}

function characterWeight(character: string) {
  if (/\s/u.test(character)) return 0.25;
  if (/^[\p{P}\p{S}]$/u.test(character)) return 0.45;
  return 1;
}

type Segmenter = {
  segment: (text: string) => Iterable<{ segment: string }>;
};

type SegmenterConstructor = new (
  locale?: string | string[],
  options?: { granularity: "grapheme" },
) => Segmenter;

function splitGraphemes(text: string) {
  const Segmenter = (Intl as typeof Intl & { Segmenter?: SegmenterConstructor }).Segmenter;
  if (!Segmenter) return Array.from(text);
  return Array.from(new Segmenter(undefined, { granularity: "grapheme" }).segment(text), (
    item,
  ) => item.segment);
}

function secondsToMilliseconds(value: number) {
  return Math.max(0, Number.isFinite(value) ? value * 1_000 : 0);
}

function isDesktopLyricsFont(value: unknown): value is DesktopLyricsFont {
  return DESKTOP_LYRICS_FONT_OPTIONS.some((option) => option.value === value);
}

function isDesktopLyricsAlignment(value: unknown): value is DesktopLyricsAlignment {
  return value === "left" || value === "center" || value === "right";
}

function isDesktopLyricsEffect(value: unknown): value is DesktopLyricsEffect {
  return value === "shadow" || value === "outline" || value === "none";
}

function isDesktopLyricsFontWeight(value: unknown): value is number {
  return value === 400 || value === 500 || value === 600 || value === 700 || value === 800;
}
