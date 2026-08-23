<script setup lang="ts">
import { Keyboard, LoaderCircle, X } from "@lucide/vue";
import { t } from "../i18n";
import {
  GLOBAL_SHORTCUT_ACTIONS,
  globalShortcutDisplayKeys,
  type GlobalShortcutAction,
  type GlobalShortcutCaptureError,
  type GlobalShortcutPreferences,
} from "../lib/global-shortcut-preferences";
import type { GlobalShortcutError } from "../composables/use-global-shortcuts";

const props = defineProps<{
  applyingAction: GlobalShortcutAction | null;
  available: boolean;
  bindings: GlobalShortcutPreferences;
  captureError: Readonly<{
    action: GlobalShortcutAction;
    error: Exclude<GlobalShortcutCaptureError, "modifier-only">;
  }> | null;
  errors: Record<GlobalShortcutAction, GlobalShortcutError | null>;
  recordingAction: GlobalShortcutAction | null;
}>();

defineEmits<{
  clear: [action: GlobalShortcutAction];
  record: [action: GlobalShortcutAction];
}>();

function rowError(action: GlobalShortcutAction): string | null {
  if (props.captureError?.action === action) {
    return props.captureError.error === "modifier-required"
      ? t("Include Control, Command, or Alt")
      : t("This key cannot be used as a system shortcut");
  }

  const error = props.errors[action];
  if (!error) return null;
  if (error.code === "duplicate" && error.conflictingAction) {
    const conflictingAction = GLOBAL_SHORTCUT_ACTIONS.find(
      (candidate) => candidate.id === error.conflictingAction,
    );
    return t("This shortcut is already assigned to {action}", {
      action: t(conflictingAction?.label ?? "another action"),
    });
  }
  if (error.code === "unavailable") {
    return t("This shortcut is unavailable. Choose another combination");
  }
  return error.code === "unregister"
    ? t("Could not update the system shortcut")
    : t("This key cannot be used as a system shortcut");
}

function recordButtonLabel(action: GlobalShortcutAction, label: string): string {
  if (props.recordingAction === action) return t("Recording system shortcut for {action}", { action: t(label) });
  const binding = props.bindings[action];
  return binding
    ? t("Change system shortcut for {action}. Current shortcut: {shortcut}", {
      action: t(label),
      shortcut: globalShortcutDisplayKeys(binding).join("+"),
    })
    : t("Record system shortcut for {action}", { action: t(label) });
}
</script>

<template>
  <section class="overflow-hidden rounded border border-base-300 bg-base-100">
    <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
      <Keyboard :size="18" aria-hidden="true" />
      <h2 class="text-base font-semibold">{{ t("System shortcuts") }}</h2>
    </div>

    <div v-if="!available" role="status" class="alert alert-info alert-soft rounded-none">
      <span class="text-sm">{{ t("System shortcuts are only available in the desktop app") }}</span>
    </div>

    <ul class="list p-0">
      <li
        v-for="action in GLOBAL_SHORTCUT_ACTIONS"
        :key="action.id"
        class="list-row flex min-h-14 flex-wrap items-center gap-2 rounded-none border-b border-base-300 px-4 py-3 last:border-b-0 sm:flex-nowrap"
      >
        <div class="list-col-grow min-w-40">
          <div class="text-sm font-medium">{{ t(action.label) }}</div>
          <div
            v-if="rowError(action.id)"
            :id="`system-shortcut-error-${action.id}`"
            role="alert"
            class="mt-0.5 text-xs text-error"
          >
            {{ rowError(action.id) }}
          </div>
        </div>

        <button
          class="btn btn-sm min-w-28 max-w-48"
          :class="{ 'btn-active': recordingAction === action.id }"
          type="button"
          :disabled="!available || applyingAction !== null"
          :aria-label="recordButtonLabel(action.id, action.label)"
          :aria-describedby="rowError(action.id) ? `system-shortcut-error-${action.id}` : undefined"
          :aria-pressed="recordingAction === action.id"
          :title="recordButtonLabel(action.id, action.label)"
          @click="$emit('record', action.id)"
        >
          <LoaderCircle
            v-if="applyingAction === action.id"
            class="animate-spin"
            :size="15"
            aria-hidden="true"
          />
          <span v-if="recordingAction === action.id">{{ t("Recording") }}</span>
          <span v-else-if="bindings[action.id]" class="flex min-w-0 items-center gap-1" aria-hidden="true">
            <template
              v-for="(key, index) in globalShortcutDisplayKeys(bindings[action.id]!)"
              :key="`${action.id}-${key}-${index}`"
            >
              <span v-if="index > 0" class="text-xs text-muted">+</span>
              <kbd class="kbd kbd-sm">{{ key }}</kbd>
            </template>
          </span>
          <template v-else>
            <Keyboard :size="15" aria-hidden="true" />
            <span>{{ t("Not set") }}</span>
          </template>
        </button>

        <div class="w-8 shrink-0">
          <button
            v-if="bindings[action.id]"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :disabled="!available || applyingAction !== null"
            :aria-label="t('Clear system shortcut for {action}', { action: t(action.label) })"
            :title="t('Clear system shortcut')"
            @click="$emit('clear', action.id)"
          >
            <X :size="15" aria-hidden="true" />
          </button>
        </div>
      </li>
    </ul>
  </section>
</template>
