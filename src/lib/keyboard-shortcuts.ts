export type ShortcutPlatform = "mac" | "other";

export type AppShortcutId =
  | "togglePlayback"
  | "previousTrack"
  | "nextTrack"
  | "seekBackward"
  | "seekForward"
  | "volumeDown"
  | "volumeUp"
  | "toggleMute"
  | "focusSearch"
  | "openSettings"
  | "showShortcuts";

export type ShortcutCategory = "Playback" | "Application";

export type ShortcutBinding = Readonly<{
  key: string;
  mod?: boolean;
  alt?: boolean;
  shift?: boolean;
  display?: boolean;
  allowInInteractive?: boolean;
}>;

export type AppShortcutDefinition = Readonly<{
  id: AppShortcutId;
  label: string;
  category: ShortcutCategory;
  bindings: readonly ShortcutBinding[];
  allowRepeat?: boolean;
}>;

export type ShortcutMatch = Readonly<{
  shortcut: AppShortcutDefinition;
  binding: ShortcutBinding;
}>;

export const APP_SHORTCUTS: readonly AppShortcutDefinition[] = [
  {
    id: "togglePlayback",
    label: "Play or pause",
    category: "Playback",
    bindings: [
      { key: " " },
      { key: "MediaPlayPause", display: false, allowInInteractive: true },
    ],
  },
  {
    id: "previousTrack",
    label: "Previous track",
    category: "Playback",
    bindings: [
      { key: "ArrowLeft", mod: true },
      { key: "MediaTrackPrevious", display: false, allowInInteractive: true },
    ],
  },
  {
    id: "nextTrack",
    label: "Next track",
    category: "Playback",
    bindings: [
      { key: "ArrowRight", mod: true },
      { key: "MediaTrackNext", display: false, allowInInteractive: true },
    ],
  },
  {
    id: "seekBackward",
    label: "Seek backward 5 seconds",
    category: "Playback",
    bindings: [{ key: "ArrowLeft" }],
    allowRepeat: true,
  },
  {
    id: "seekForward",
    label: "Seek forward 5 seconds",
    category: "Playback",
    bindings: [{ key: "ArrowRight" }],
    allowRepeat: true,
  },
  {
    id: "volumeDown",
    label: "Decrease volume",
    category: "Playback",
    bindings: [{ key: "ArrowDown" }],
    allowRepeat: true,
  },
  {
    id: "volumeUp",
    label: "Increase volume",
    category: "Playback",
    bindings: [{ key: "ArrowUp" }],
    allowRepeat: true,
  },
  {
    id: "toggleMute",
    label: "Mute or unmute",
    category: "Playback",
    bindings: [{ key: "m" }],
  },
  {
    id: "focusSearch",
    label: "Open search",
    category: "Application",
    bindings: [{ key: "k", mod: true, allowInInteractive: true }],
  },
  {
    id: "openSettings",
    label: "Open settings",
    category: "Application",
    bindings: [{ key: ",", mod: true, allowInInteractive: true }],
  },
  {
    id: "showShortcuts",
    label: "Show keyboard shortcuts",
    category: "Application",
    bindings: [{ key: "/", mod: true, allowInInteractive: true }],
  },
];

export const SHORTCUT_CATEGORIES: readonly ShortcutCategory[] = ["Playback", "Application"];

export function detectShortcutPlatform(platformDescription?: string): ShortcutPlatform {
  const description = platformDescription ?? (
    typeof navigator === "undefined" ? "" : `${navigator.platform} ${navigator.userAgent}`
  );
  return /Mac|iPhone|iPad|iPod/i.test(description) ? "mac" : "other";
}

export function matchKeyboardShortcut(
  event: KeyboardEvent,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): ShortcutMatch | null {
  for (const shortcut of APP_SHORTCUTS) {
    for (const binding of shortcut.bindings) {
      if (bindingMatches(event, binding, platform)) {
        return { shortcut, binding };
      }
    }
  }
  return null;
}

export function shortcutAriaKeys(
  ids: AppShortcutId | readonly AppShortcutId[],
  platform: ShortcutPlatform = detectShortcutPlatform(),
): string {
  const requestedIds = Array.isArray(ids) ? ids : [ids];
  return requestedIds
    .flatMap((id) => shortcutDefinition(id).bindings)
    .map((binding) => formatAriaBinding(binding, platform))
    .join(" ");
}

export function shortcutDisplayBindings(
  id: AppShortcutId,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): readonly (readonly string[])[] {
  return shortcutDefinition(id).bindings
    .filter((binding) => binding.display !== false)
    .map((binding) => formatDisplayBinding(binding, platform));
}

export function shortcutHint(
  id: AppShortcutId,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): string {
  return shortcutDisplayBindings(id, platform)[0]?.join("+") ?? "";
}

export function isInteractiveShortcutTarget(target: EventTarget | null): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;

  return Boolean(target.closest([
    "input",
    "textarea",
    "select",
    "button",
    "a[href]",
    "summary",
    "[contenteditable]:not([contenteditable='false'])",
    "[tabindex]:not([tabindex='-1'])",
    "[role='textbox']",
    "[role='combobox']",
    "[role='grid']",
    "[role='listbox']",
    "[role='menu']",
    "[role='slider']",
    "[role='spinbutton']",
    "[role='table']",
    "[role='tablist']",
    "[role='tree']",
  ].join(",")));
}

function shortcutDefinition(id: AppShortcutId): AppShortcutDefinition {
  const shortcut = APP_SHORTCUTS.find((candidate) => candidate.id === id);
  if (!shortcut) throw new Error(`Unknown keyboard shortcut: ${id}`);
  return shortcut;
}

function bindingMatches(
  event: KeyboardEvent,
  binding: ShortcutBinding,
  platform: ShortcutPlatform,
): boolean {
  const expectedControl = Boolean(binding.mod && platform === "other");
  const expectedMeta = Boolean(binding.mod && platform === "mac");
  return normalizeKey(event.key) === normalizeKey(binding.key)
    && event.ctrlKey === expectedControl
    && event.metaKey === expectedMeta
    && event.altKey === Boolean(binding.alt)
    && event.shiftKey === Boolean(binding.shift);
}

function normalizeKey(key: string): string {
  return key.length === 1 ? key.toLocaleLowerCase() : key;
}

function formatAriaBinding(binding: ShortcutBinding, platform: ShortcutPlatform): string {
  const keys: string[] = [];
  if (binding.mod) keys.push(platform === "mac" ? "Meta" : "Control");
  if (binding.alt) keys.push("Alt");
  if (binding.shift) keys.push("Shift");
  keys.push(binding.key === " " ? "Space" : binding.key);
  return keys.join("+");
}

function formatDisplayBinding(
  binding: ShortcutBinding,
  platform: ShortcutPlatform,
): readonly string[] {
  const keys: string[] = [];
  if (binding.mod) keys.push(platform === "mac" ? "Cmd" : "Ctrl");
  if (binding.alt) keys.push(platform === "mac" ? "Option" : "Alt");
  if (binding.shift) keys.push("Shift");
  keys.push(displayKey(binding.key));
  return keys;
}

function displayKey(key: string): string {
  switch (key) {
    case " ":
      return "Space";
    case "ArrowLeft":
      return "Left";
    case "ArrowRight":
      return "Right";
    case "ArrowUp":
      return "Up";
    case "ArrowDown":
      return "Down";
    default:
      return key.length === 1 ? key.toLocaleUpperCase() : key;
  }
}
