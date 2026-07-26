import { emitTo } from "@tauri-apps/api/event";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  DESKTOP_LYRICS_STATE_EVENT,
  DESKTOP_LYRICS_WINDOW_LABEL,
  desktopLyricsMinimumHeight,
  type DesktopLyricsState,
} from "./desktop-lyrics";

export function hasDesktopWindowRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function syncDesktopLyricsWindow(state: DesktopLyricsState) {
  if (!hasDesktopWindowRuntime()) return;

  const desktopWindow = await WebviewWindow.getByLabel(DESKTOP_LYRICS_WINDOW_LABEL);
  if (!desktopWindow) return;

  await ignoreUnsupported(() =>
    desktopWindow.setMinSize(
      new LogicalSize(320, desktopLyricsMinimumHeight(state.preferences)),
    ),
  );
  await ignoreUnsupported(() => desktopWindow.setAlwaysOnTop(state.preferences.alwaysOnTop));
  await ignoreUnsupported(() => desktopWindow.setIgnoreCursorEvents(state.preferences.locked));

  if (!state.preferences.enabled) {
    await ignoreUnsupported(() => desktopWindow.hide());
    return;
  }

  await ignoreUnsupported(() => desktopWindow.show());
  await broadcastDesktopLyricsState(state);
}

export async function broadcastDesktopLyricsState(state: DesktopLyricsState) {
  if (!hasDesktopWindowRuntime() || !state.preferences.enabled) return;
  await ignoreUnsupported(() =>
    emitTo(DESKTOP_LYRICS_WINDOW_LABEL, DESKTOP_LYRICS_STATE_EVENT, state),
  );
}

async function ignoreUnsupported(action: () => Promise<unknown>) {
  try {
    await action();
  } catch {
    // Window managers differ in their support for overlay hints; keep the lyrics usable.
  }
}
