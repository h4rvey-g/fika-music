<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  AlertCircle,
  CircleCheck,
  RefreshCw,
  Save,
  Trash2,
} from "@lucide/vue";
import {
  clearChkszApiKey,
  getChkszApiKeyStatus,
  setChkszApiKey,
} from "../lib/chksz-audio-source-api";
import { normalizeError } from "../lib/errors";
import { t } from "../i18n";

const apiKey = ref("");
const configured = ref<boolean | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);
const notice = ref<string | null>(null);

onMounted(async () => {
  try {
    configured.value = await getChkszApiKeyStatus();
  } catch (reason) {
    error.value = normalizeError(reason);
  }
});

async function saveApiKey() {
  const value = apiKey.value.trim();
  if (!value) {
    error.value = t("API key is required.");
    return;
  }
  busy.value = true;
  error.value = null;
  notice.value = null;
  try {
    await setChkszApiKey(value);
    apiKey.value = "";
    configured.value = true;
    notice.value = t("ChKSz API key saved.");
  } catch (reason) {
    error.value = normalizeError(reason);
  } finally {
    busy.value = false;
  }
}

async function clearApiKey() {
  if (!window.confirm(t("Clear ChKSz API key?"))) return;
  busy.value = true;
  error.value = null;
  notice.value = null;
  try {
    await clearChkszApiKey();
    apiKey.value = "";
    configured.value = false;
    notice.value = t("ChKSz API key cleared.");
  } catch (reason) {
    error.value = normalizeError(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="space-y-3" data-testid="chksz-audio-source-settings">
    <div class="flex items-center justify-between gap-3">
      <h4 class="text-sm font-semibold">{{ t("ChKSz API") }}</h4>
      <span
        class="badge badge-sm"
        :class="configured ? 'badge-success' : 'badge-ghost'"
      >
        {{ configured === null ? t("Loading") : t(configured ? "Configured" : "Not configured") }}
      </span>
    </div>

    <form
      class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-end"
      @submit.prevent="saveApiKey"
    >
      <fieldset class="fieldset min-w-0">
        <legend class="fieldset-legend">{{ t("API key") }}</legend>
        <input
          v-model="apiKey"
          class="input input-sm w-full"
          type="password"
          autocomplete="off"
          spellcheck="false"
          placeholder="chksz_..."
          :aria-label="t('ChKSz API key')"
          :disabled="busy"
        />
      </fieldset>
      <button class="btn btn-primary btn-sm" type="submit" :disabled="busy">
        <RefreshCw v-if="busy" class="animate-spin" :size="16" aria-hidden="true" />
        <Save v-else :size="16" aria-hidden="true" />
        {{ t("Save") }}
      </button>
      <button
        v-if="configured"
        class="btn btn-ghost btn-sm text-error"
        type="button"
        :disabled="busy"
        @click="clearApiKey"
      >
        <Trash2 :size="16" aria-hidden="true" />
        {{ t("Clear API key") }}
      </button>
    </form>

    <div v-if="error" role="alert" class="alert alert-error alert-soft py-2 text-sm">
      <AlertCircle :size="17" aria-hidden="true" />
      <span>{{ error }}</span>
    </div>
    <div v-if="notice" role="status" class="alert alert-success alert-soft py-2 text-sm">
      <CircleCheck :size="17" aria-hidden="true" />
      <span>{{ notice }}</span>
    </div>
  </div>
</template>
