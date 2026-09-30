<script setup lang="ts">
import { onMounted, ref } from "vue";
import { ExternalLink, Globe, RefreshCw, ShieldCheck, X } from "@lucide/vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { normalizeError } from "../lib/errors";
import { t } from "../i18n";
import type { NeteaseVerificationNotice } from "../lib/netease-verification";

const props = defineProps<{ verification: NeteaseVerificationNotice; webLoginSupported?: boolean }>();
const emit = defineEmits<{ close: []; retry: []; useWeb: [] }>();
const dialog = ref<HTMLDialogElement | null>(null);
const isOpening = ref(false);
const error = ref<string | null>(null);

onMounted(() => {
  if (typeof dialog.value?.showModal === "function") dialog.value.showModal();
  else dialog.value?.setAttribute("open", "");
});

async function openVerification() {
  if (!props.verification.challenge || isOpening.value) return;
  isOpening.value = true;
  error.value = null;
  try {
    await openUrl(props.verification.challenge.url);
  } catch (cause) {
    error.value = normalizeError(cause);
  } finally {
    isOpening.value = false;
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby="netease-verification-title"
    aria-describedby="netease-verification-description"
    @cancel.prevent="emit('close')"
  >
    <div class="modal-box max-w-md! rounded p-5">
      <div class="flex items-center gap-2">
        <ShieldCheck :size="20" class="shrink-0 text-warning" aria-hidden="true" />
        <h3 id="netease-verification-title" class="min-w-0 flex-1 text-base font-semibold">
          {{ t("NetEase security verification") }}
        </h3>
        <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Close')" :title="t('Close')" @click="emit('close')">
          <X :size="16" aria-hidden="true" />
        </button>
      </div>
      <p class="mt-3 break-words text-sm">{{ verification.message }}</p>
      <div id="netease-verification-description" class="mt-3 text-sm text-muted">
        <template v-if="verification.challenge">
          {{ t("Complete the security check in NetEase Cloud Music, then retry.") }}
        </template>
        <template v-else>
          {{ t("No usable verification challenge was returned. Complete the security check in NetEase Cloud Music, or sign in on the official website.") }}
        </template>
      </div>
      <img
        v-if="verification.challenge"
        :src="verification.challenge.qrImageDataUrl"
        class="mx-auto mt-4 size-56 max-w-full rounded bg-white"
        :alt="t('NetEase security verification QR code')"
        width="224"
        height="224"
      />
      <div v-if="error" role="alert" class="alert alert-error mt-4 break-words text-sm">{{ error }}</div>
      <div class="mt-5 flex flex-wrap justify-end gap-2">
        <button v-if="webLoginSupported" class="btn btn-sm" type="button" @click="emit('useWeb')">
          <Globe :size="16" aria-hidden="true" />
          {{ t("Use official website login") }}
        </button>
        <button class="btn btn-sm" type="button" @click="emit('retry')">
          <RefreshCw :size="16" aria-hidden="true" />
          {{ t("Retry") }}
        </button>
        <button v-if="verification.challenge" class="btn btn-primary btn-sm" type="button" :disabled="isOpening" @click="openVerification">
          <ExternalLink :size="16" aria-hidden="true" />
          {{ t("Open verification") }}
        </button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop" @submit.prevent="emit('close')">
      <button type="submit">{{ t("Close") }}</button>
    </form>
  </dialog>
</template>
