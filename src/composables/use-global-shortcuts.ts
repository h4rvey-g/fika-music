import { isTauri } from "@tauri-apps/api/core";
import {
  register as registerGlobalShortcut,
  unregister as unregisterGlobalShortcut,
  type ShortcutEvent,
} from "@tauri-apps/plugin-global-shortcut";
import { ref } from "vue";
import {
  GLOBAL_SHORTCUT_ACTIONS,
  isValidGlobalShortcut,
  loadGlobalShortcutPreferences,
  saveGlobalShortcutPreferences,
  type GlobalShortcutAction,
  type GlobalShortcutPreferences,
} from "../lib/global-shortcut-preferences";

export type GlobalShortcutError = Readonly<{
  code: "duplicate" | "invalid" | "unavailable" | "unregister";
  conflictingAction?: GlobalShortcutAction;
}>;

export type GlobalShortcutDependencies = Readonly<{
  isTauri: () => boolean;
  register: (shortcut: string, handler: (event: ShortcutEvent) => void) => Promise<void>;
  unregister: (shortcut: string) => Promise<void>;
}>;

type ShortcutStorage = Pick<Storage, "getItem" | "setItem">;

const defaultDependencies: GlobalShortcutDependencies = {
  isTauri,
  register: registerGlobalShortcut,
  unregister: unregisterGlobalShortcut,
};

export function useGlobalShortcuts(
  handler: (action: GlobalShortcutAction) => void,
  dependencies: GlobalShortcutDependencies = defaultDependencies,
  storage?: ShortcutStorage | null,
) {
  const bindings = ref<GlobalShortcutPreferences>(
    loadGlobalShortcutPreferences(storage === undefined ? undefined : storage),
  );
  const errors = ref(createErrorState());
  const applyingAction = ref<GlobalShortcutAction | null>(null);
  const available = dependencies.isTauri();
  const activeBindings = new Map<GlobalShortcutAction, string>();
  let disposed = false;

  async function initialize() {
    disposed = false;
    if (!available) return;

    for (const action of GLOBAL_SHORTCUT_ACTIONS) {
      const shortcut = bindings.value[action.id];
      if (!shortcut) continue;
      try {
        await register(action.id, shortcut);
        activeBindings.set(action.id, shortcut);
      } catch {
        setError(action.id, { code: "unavailable" });
      }
    }
  }

  async function setBinding(action: GlobalShortcutAction, shortcut: string): Promise<boolean> {
    if (!available || applyingAction.value) return false;
    if (!isValidGlobalShortcut(shortcut)) {
      setError(action, { code: "invalid" });
      return false;
    }

    const conflictingAction = GLOBAL_SHORTCUT_ACTIONS.find((candidate) =>
      candidate.id !== action && bindings.value[candidate.id] === shortcut)?.id;
    if (conflictingAction) {
      setError(action, { code: "duplicate", conflictingAction });
      return false;
    }

    const previousShortcut = bindings.value[action];
    const activeShortcut = activeBindings.get(action);
    if (previousShortcut === shortcut && activeShortcut === shortcut) {
      setError(action, null);
      return true;
    }

    applyingAction.value = action;
    setError(action, null);
    try {
      await register(action, shortcut);
    } catch {
      setError(action, { code: "unavailable" });
      applyingAction.value = null;
      return false;
    }

    if (activeShortcut && activeShortcut !== shortcut) {
      try {
        await dependencies.unregister(activeShortcut);
      } catch {
        await dependencies.unregister(shortcut).catch(() => undefined);
        setError(action, { code: "unregister" });
        applyingAction.value = null;
        return false;
      }
    }

    activeBindings.set(action, shortcut);
    bindings.value = { ...bindings.value, [action]: shortcut };
    persist();
    applyingAction.value = null;
    return true;
  }

  async function clearBinding(action: GlobalShortcutAction): Promise<boolean> {
    if (!available || applyingAction.value) return false;
    applyingAction.value = action;
    setError(action, null);
    const activeShortcut = activeBindings.get(action);
    if (activeShortcut) {
      try {
        await dependencies.unregister(activeShortcut);
      } catch {
        setError(action, { code: "unregister" });
        applyingAction.value = null;
        return false;
      }
    }

    activeBindings.delete(action);
    bindings.value = { ...bindings.value, [action]: null };
    persist();
    applyingAction.value = null;
    return true;
  }

  async function dispose() {
    disposed = true;
    const shortcuts = [...activeBindings.values()];
    activeBindings.clear();
    await Promise.allSettled(shortcuts.map((shortcut) => dependencies.unregister(shortcut)));
  }

  function register(action: GlobalShortcutAction, shortcut: string) {
    return dependencies.register(shortcut, (event) => {
      if (!disposed && event.state === "Pressed") handler(action);
    });
  }

  function persist() {
    saveGlobalShortcutPreferences(bindings.value, storage === undefined ? undefined : storage);
  }

  function setError(action: GlobalShortcutAction, error: GlobalShortcutError | null) {
    errors.value = { ...errors.value, [action]: error };
  }

  return {
    applyingAction,
    available,
    bindings,
    clearError: (action: GlobalShortcutAction) => setError(action, null),
    clearBinding,
    dispose,
    errors,
    initialize,
    setBinding,
  };
}

function createErrorState(): Record<GlobalShortcutAction, GlobalShortcutError | null> {
  return {
    togglePlayback: null,
    previousTrack: null,
    nextTrack: null,
    seekBackward: null,
    seekForward: null,
    volumeDown: null,
    volumeUp: null,
    toggleMute: null,
  };
}
