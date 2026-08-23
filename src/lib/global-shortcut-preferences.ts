import { detectShortcutPlatform, type AppShortcutId, type ShortcutPlatform } from "./keyboard-shortcuts";

export const GLOBAL_SHORTCUT_ACTIONS = [
  { id: "togglePlayback", label: "Play or pause" },
  { id: "previousTrack", label: "Previous track" },
  { id: "nextTrack", label: "Next track" },
  { id: "seekBackward", label: "Seek backward 5 seconds" },
  { id: "seekForward", label: "Seek forward 5 seconds" },
  { id: "volumeDown", label: "Decrease volume" },
  { id: "volumeUp", label: "Increase volume" },
  { id: "toggleMute", label: "Mute or unmute" },
] as const satisfies readonly { id: AppShortcutId; label: string }[];

export type GlobalShortcutAction = (typeof GLOBAL_SHORTCUT_ACTIONS)[number]["id"];
export type GlobalShortcutPreferences = Record<GlobalShortcutAction, string | null>;
export type GlobalShortcutCaptureError = "modifier-only" | "modifier-required" | "unsupported-key";
export type GlobalShortcutCaptureResult =
  | Readonly<{ shortcut: string; error: null }>
  | Readonly<{ shortcut: null; error: GlobalShortcutCaptureError }>;

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export const GLOBAL_SHORTCUTS_STORAGE_KEY = "fika.global-shortcuts";

export const DEFAULT_GLOBAL_SHORTCUT_PREFERENCES: GlobalShortcutPreferences = {
  togglePlayback: null,
  previousTrack: null,
  nextTrack: null,
  seekBackward: null,
  seekForward: null,
  volumeDown: null,
  volumeUp: null,
  toggleMute: null,
};

const MODIFIER_CODES = new Set([
  "AltLeft",
  "AltRight",
  "ControlLeft",
  "ControlRight",
  "MetaLeft",
  "MetaRight",
  "ShiftLeft",
  "ShiftRight",
]);

const NAMED_KEYS = new Set([
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "Backquote",
  "Backslash",
  "Backspace",
  "BracketLeft",
  "BracketRight",
  "Comma",
  "Delete",
  "End",
  "Enter",
  "Equal",
  "Home",
  "Insert",
  "Minus",
  "Numpad0",
  "Numpad1",
  "Numpad2",
  "Numpad3",
  "Numpad4",
  "Numpad5",
  "Numpad6",
  "Numpad7",
  "Numpad8",
  "Numpad9",
  "NumpadAdd",
  "NumpadDecimal",
  "NumpadDivide",
  "NumpadEnter",
  "NumpadEqual",
  "NumpadMultiply",
  "NumpadSubtract",
  "PageDown",
  "PageUp",
  "Period",
  "Quote",
  "Semicolon",
  "Slash",
  "Space",
  "Tab",
]);

const MODIFIER_TOKENS = new Set(["Alt", "CommandOrControl", "Control", "Shift", "Super"]);

export function loadGlobalShortcutPreferences(
  storage: ReadableStorage | null = browserStorage(),
): GlobalShortcutPreferences {
  if (!storage) return { ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES };

  try {
    const storedValue = storage.getItem(GLOBAL_SHORTCUTS_STORAGE_KEY);
    return storedValue
      ? parseGlobalShortcutPreferences(JSON.parse(storedValue))
      : { ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES };
  } catch {
    return { ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES };
  }
}

export function saveGlobalShortcutPreferences(
  preferences: GlobalShortcutPreferences,
  storage: WritableStorage | null = browserStorage(),
) {
  if (!storage) return;

  try {
    storage.setItem(
      GLOBAL_SHORTCUTS_STORAGE_KEY,
      JSON.stringify(parseGlobalShortcutPreferences(preferences)),
    );
  } catch {
    // Shortcut preferences should not prevent the application from rendering.
  }
}

export function parseGlobalShortcutPreferences(value: unknown): GlobalShortcutPreferences {
  const candidate = value && typeof value === "object"
    ? value as Partial<Record<GlobalShortcutAction, unknown>>
    : {};
  const parsed = { ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES };
  const assigned = new Set<string>();

  for (const action of GLOBAL_SHORTCUT_ACTIONS) {
    const shortcut = candidate[action.id];
    if (typeof shortcut !== "string" || !isValidGlobalShortcut(shortcut) || assigned.has(shortcut)) {
      continue;
    }
    parsed[action.id] = shortcut;
    assigned.add(shortcut);
  }
  return parsed;
}

export function captureGlobalShortcut(
  event: Pick<KeyboardEvent, "altKey" | "code" | "ctrlKey" | "metaKey" | "shiftKey">,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): GlobalShortcutCaptureResult {
  if (MODIFIER_CODES.has(event.code)) {
    return { shortcut: null, error: "modifier-only" };
  }
  if (!isSupportedKeyCode(event.code)) {
    return { shortcut: null, error: "unsupported-key" };
  }

  const modifiers: string[] = [];
  if (platform === "mac") {
    if (event.metaKey) modifiers.push("CommandOrControl");
    if (event.ctrlKey) modifiers.push("Control");
  } else {
    if (event.ctrlKey) modifiers.push("CommandOrControl");
    if (event.metaKey) modifiers.push("Super");
  }
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (!event.altKey && !event.ctrlKey && !event.metaKey) {
    return { shortcut: null, error: "modifier-required" };
  }
  return { shortcut: [...modifiers, event.code].join("+"), error: null };
}

export function globalShortcutDisplayKeys(
  shortcut: string,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): readonly string[] {
  return shortcut.split("+").map((token) => displayToken(token, platform));
}

export function globalShortcutAriaKeys(
  shortcut: string,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): string {
  return shortcut
    .split("+")
    .map((token) => ariaToken(token, platform))
    .join("+");
}

export function isValidGlobalShortcut(shortcut: string): boolean {
  if (shortcut.length > 100) return false;
  const tokens = shortcut.split("+");
  if (tokens.length < 2 || !isSupportedKeyCode(tokens[tokens.length - 1])) return false;
  const modifiers = tokens.slice(0, -1);
  return modifiers.length > 0
    && modifiers.some((token) => token !== "Shift")
    && modifiers.every((token) => MODIFIER_TOKENS.has(token))
    && new Set(modifiers).size === modifiers.length;
}

function isSupportedKeyCode(code: string): boolean {
  return NAMED_KEYS.has(code)
    || /^Key[A-Z]$/.test(code)
    || /^Digit[0-9]$/.test(code)
    || /^F(?:[1-9]|1[0-9]|2[0-4])$/.test(code);
}

function displayToken(token: string, platform: ShortcutPlatform): string {
  if (token === "CommandOrControl") return platform === "mac" ? "Cmd" : "Ctrl";
  if (token === "Super") return platform === "mac" ? "Cmd" : "Super";
  if (token === "Control") return "Ctrl";
  return displayKey(token);
}

function ariaToken(token: string, platform: ShortcutPlatform): string {
  if (token === "CommandOrControl") return platform === "mac" ? "Meta" : "Control";
  if (token === "Super") return "Meta";
  return token === "Control" ? "Control" : displayKey(token);
}

function displayKey(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (code.startsWith("Arrow")) return code.slice(5);
  const labels: Record<string, string> = {
    Backquote: "`",
    Backslash: "\\",
    BracketLeft: "[",
    BracketRight: "]",
    Comma: ",",
    Equal: "=",
    Minus: "-",
    Period: ".",
    Quote: "'",
    Semicolon: ";",
    Slash: "/",
  };
  return labels[code] ?? code;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
