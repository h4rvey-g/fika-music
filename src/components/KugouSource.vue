<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useMutation, useQuery } from "@tanstack/vue-query";
import {
  AlertCircle,
  CircleCheck,
  Clock3,
  ListMusic,
  LogIn,
  LogOut,
  MessageSquareText,
  Power,
  QrCode,
  RefreshCw,
  UserRound,
  X,
} from "@lucide/vue";
import { listPlugins } from "../lib/plugin-api";
import { usePhoneLoginSession, useQrLoginSession } from "../composables/source-workspace";
import { normalizeError, queryError } from "../lib/errors";
import { t } from "../i18n";
import type {
  AudioSourceId,
  AudioSourceOption,
} from "../lib/audio-source-api";
import {
  KUGOU_PLUGIN_ID,
  cancelKugouPhoneLogin,
  cancelKugouQrLogin,
  completeKugouPhoneLogin,
  disconnectKugouAccount,
  listKugouAccounts,
  pollKugouQrLogin,
  startKugouPhoneLogin,
  startKugouQrLogin,
} from "../lib/kugou-api";

const props = defineProps<{
  playbackSource: AudioSourceId;
  audioSources: AudioSourceOption[];
  automaticSourceSelection?: boolean;
}>();

const emit = defineEmits<{
  "update:playbackSource": [source: AudioSourceId];
  openPlugins: [];
  openAudioSources: [];
}>();

const activeAccountRef = ref("");
const manualError = ref<string | null>(null);
const dismissedQueryError = ref("");
const sourceNotice = ref<string | null>(null);
const showLogin = ref(false);
const loginMode = ref<"phone" | "qr">("phone");
const phone = ref("");
const verificationCode = ref("");

const pluginsQuery = useQuery({
  queryKey: ["plugins"],
  queryFn: listPlugins,
  staleTime: 0,
});

const accountsQuery = useQuery({
  queryKey: ["kugou", "accounts"],
  queryFn: listKugouAccounts,
  staleTime: 0,
});

const disconnectMutation = useMutation({
  mutationFn: (accountRef: string) => disconnectKugouAccount(accountRef),
});

const plugin = computed(
  () => pluginsQuery.data.value?.find((record) => record.id === KUGOU_PLUGIN_ID) ?? null,
);
const accounts = computed(() => accountsQuery.data.value ?? []);
const isPluginReady = computed(
  () => plugin.value?.enabled === true && plugin.value.state === "enabled",
);
const activeAccount = computed(
  () => accounts.value.find((account) => account.accountRef === activeAccountRef.value) ?? null,
);

const queryErrorMessage = computed(() => {
  const errors = [
    queryError(t("Plugin"), pluginsQuery.isError.value, pluginsQuery.error.value),
    queryError(t("Accounts"), accountsQuery.isError.value, accountsQuery.error.value),
  ].filter((message): message is string => Boolean(message));
  return errors.join(" ");
});

const sourceError = computed<string | null>({
  get() {
    if (manualError.value) {
      return manualError.value;
    }
    const queryError = queryErrorMessage.value;
    return queryError && queryError !== dismissedQueryError.value ? queryError : null;
  },
  set(value) {
    if (value === null) {
      manualError.value = null;
      dismissedQueryError.value = queryErrorMessage.value;
    } else {
      manualError.value = value;
    }
  },
});

const qrSession = useQrLoginSession({
  providerName: "KuGou Music",
  start: startKugouQrLogin,
  poll: pollKugouQrLogin,
  cancel: cancelKugouQrLogin,
  onConnected: connectAccount,
  onError: (error) => {
    sourceError.value = normalizeError(error);
  },
});
const qrLogin = qrSession.login;
const qrStatus = qrSession.status;
const isConnectingQr = qrSession.isConnecting;
const isPollingQr = qrSession.isPolling;
const cancelQrLogin = qrSession.cancel;

const phoneSession = usePhoneLoginSession({
  start: startKugouPhoneLogin,
  complete: completeKugouPhoneLogin,
  cancel: cancelKugouPhoneLogin,
  onConnected: connectAccount,
  onError: (error) => {
    sourceError.value = normalizeError(error);
  },
});
const phoneLogin = phoneSession.login;
const isSendingCode = phoneSession.isSending;
const isCompletingPhoneLogin = phoneSession.isCompleting;
const resendSeconds = phoneSession.resendSeconds;
const isPhoneValid = computed(() => /^1\d{10}$/.test(phone.value.trim()));
const isVerificationCodeValid = computed(() => /^\d{4,8}$/.test(verificationCode.value.trim()));
const maskedPhone = computed(() =>
  phone.value.trim().replace(/^(\d{3})\d{4}(\d{4})$/, "$1****$2"),
);

watch(
  () => accountsQuery.data.value,
  (connectedAccounts) => {
    if (!connectedAccounts) {
      return;
    }
    if (connectedAccounts.some((account) => account.accountRef === activeAccountRef.value)) {
      return;
    }
    activeAccountRef.value =
      connectedAccounts.find((account) => account.status === "active")?.accountRef ??
      connectedAccounts[0]?.accountRef ??
      "";
  },
  { immediate: true },
);

watch(queryErrorMessage, (message, previousMessage) => {
  if (message !== previousMessage) {
    dismissedQueryError.value = "";
  }
});

onBeforeUnmount(() => {
  qrSession.cancel();
  phoneSession.cancel();
});

async function startQrLogin() {
  if (!isPluginReady.value || isConnectingQr.value) {
    return;
  }
  sourceError.value = null;
  sourceNotice.value = null;
  await qrSession.start();
}

function openLogin() {
  if (!isPluginReady.value) return;
  sourceError.value = null;
  sourceNotice.value = null;
  showLogin.value = true;
  loginMode.value = "phone";
}

function closeLogin() {
  qrSession.cancel();
  phoneSession.cancel();
  showLogin.value = false;
  loginMode.value = "phone";
  phone.value = "";
  verificationCode.value = "";
}

function selectLoginMode(mode: "phone" | "qr") {
  if (loginMode.value === mode) return;
  if (mode === "phone") {
    qrSession.cancel();
  } else {
    phoneSession.cancel();
    verificationCode.value = "";
  }
  sourceError.value = null;
  sourceNotice.value = null;
  loginMode.value = mode;
}

async function sendVerificationCode() {
  if (!isPhoneValid.value) return;
  sourceError.value = null;
  sourceNotice.value = null;
  await phoneSession.sendCode(phone.value.trim());
  if (phoneLogin.value) {
    sourceNotice.value = t("Verification code sent to +86 {phone}.", {
      phone: maskedPhone.value,
    });
  }
}

async function submitPhoneLogin() {
  if (!isVerificationCodeValid.value) return;
  sourceError.value = null;
  sourceNotice.value = null;
  await phoneSession.complete(verificationCode.value.trim());
}

function changePhoneNumber() {
  phoneSession.cancel();
  verificationCode.value = "";
  sourceNotice.value = null;
}

async function connectAccount(account: { accountRef: string; displayName: string }) {
  await accountsQuery.refetch();
  activeAccountRef.value = account.accountRef;
  showLogin.value = false;
  phone.value = "";
  verificationCode.value = "";
  sourceNotice.value = t("{name} connected.", { name: account.displayName });
}

async function disconnectAccount() {
  const account = activeAccount.value;
  if (!account || !window.confirm(t("Disconnect {name}?", { name: account.displayName }))) {
    return;
  }
  manualError.value = null;
  try {
    await disconnectMutation.mutateAsync(account.accountRef);
    await accountsQuery.refetch();
    sourceNotice.value = t("{name} disconnected.", { name: account.displayName });
  } catch (error) {
    manualError.value = normalizeError(error);
    await refreshAccountStatuses();
  }
}

function selectPlaybackSource(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (props.audioSources.some((source) => source.value === value)) {
    emit("update:playbackSource", value);
  }
}

async function refreshAccountStatuses() {
  try {
    await accountsQuery.refetch();
  } catch {
    // Preserve the primary account error.
  }
}
</script>

<template>
  <section class="overflow-hidden rounded border border-base-300 bg-base-100">
    <header class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 2xl:flex-row 2xl:items-center 2xl:justify-between">
      <div class="flex min-w-0 items-center gap-3">
        <div class="flex size-10 shrink-0 items-center justify-center rounded bg-neutral text-neutral-content">
          <ListMusic :size="19" aria-hidden="true" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h2 class="text-base font-semibold">KuGou Music</h2>
            <span v-if="plugin" class="badge badge-sm" :class="isPluginReady ? 'badge-success' : 'badge-warning'">
              {{ isPluginReady ? t("Ready") : t("Plugin disabled") }}
            </span>
          </div>
          <p v-if="activeAccount" class="mt-0.5 truncate text-xs text-muted">
            {{ activeAccount.displayName }} · {{ t(activeAccount.status) }}
          </p>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <span v-if="automaticSourceSelection" class="badge badge-sm" :aria-label="t('Automatic Audio Source selection')">
          {{ t("Auto") }}
        </span>
        <select
          v-else
          :value="playbackSource"
          class="select select-sm w-32 max-w-full"
          :disabled="!audioSources.length"
          :aria-label="t('KuGou playback source')"
          :title="t('Playback source')"
          @change="selectPlaybackSource"
        >
          <option v-if="!audioSources.length" value="">{{ t("No audio source") }}</option>
          <option v-for="source in audioSources" :key="source.value" :value="source.value">
            {{ source.label }}
          </option>
        </select>
        <select
          v-if="accounts.length"
          v-model="activeAccountRef"
          class="select select-sm w-32 max-w-full sm:w-44"
          :aria-label="t('KuGou account')"
        >
          <option v-for="account in accounts" :key="account.accountRef" :value="account.accountRef">
            {{ account.displayName }}{{ account.status === "expired" ? t(" (expired)") : "" }}
          </option>
        </select>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!isPluginReady || showLogin || isConnectingQr || isSendingCode || isCompletingPhoneLogin"
          @click="openLogin"
        >
          <LogIn :size="16" aria-hidden="true" />
          {{ t("Connect") }}
        </button>
        <button
          v-if="activeAccount"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="t('Disconnect KuGou account')"
          :title="t('Disconnect account')"
          @click="disconnectAccount"
        >
          <LogOut :size="16" aria-hidden="true" />
        </button>
      </div>
    </header>

    <div v-if="sourceError" role="alert" class="alert alert-error m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceError }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Dismiss KuGou error')" @click="sourceError = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="sourceNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceNotice }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Dismiss KuGou notice')" @click="sourceNotice = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="isPluginReady && !audioSources.length" role="status" class="alert alert-warning alert-soft m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ t("No enabled audio source is available.") }}</span>
      <button class="btn btn-sm" type="button" @click="emit('openAudioSources')">{{ t("Open Audio Sources") }}</button>
    </div>

    <div v-if="!isPluginReady" class="flex flex-col gap-3 p-5 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex items-start gap-3">
        <Power class="mt-0.5 shrink-0 text-warning" :size="18" aria-hidden="true" />
        <div>
          <div class="text-sm font-medium">{{ t("Plugin is disabled") }}</div>
          <div class="mt-1 text-xs text-muted">{{ t("Enable the bundled Plugin to use KuGou.") }}</div>
        </div>
      </div>
      <button class="btn btn-sm" type="button" @click="emit('openPlugins')">{{ t("Open Plugins") }}</button>
    </div>

    <div v-else-if="showLogin" class="p-5">
      <div role="tablist" class="tabs tabs-box w-full sm:w-fit" :aria-label="t('KuGou login method')">
        <button
          role="tab"
          class="tab flex-1 gap-2 sm:flex-none"
          :class="{ 'tab-active': loginMode === 'phone' }"
          type="button"
          :aria-selected="loginMode === 'phone'"
          @click="selectLoginMode('phone')"
        >
          <MessageSquareText :size="16" aria-hidden="true" />
          {{ t("Verification code") }}
        </button>
        <button
          role="tab"
          class="tab flex-1 gap-2 sm:flex-none"
          :class="{ 'tab-active': loginMode === 'qr' }"
          type="button"
          :aria-selected="loginMode === 'qr'"
          @click="selectLoginMode('qr')"
        >
          <QrCode :size="16" aria-hidden="true" />
          {{ t("QR code") }}
        </button>
      </div>

      <form v-if="loginMode === 'phone'" class="mt-5 max-w-md" @submit.prevent="submitPhoneLogin">
        <fieldset class="fieldset">
          <legend class="fieldset-legend">{{ t("Phone number") }}</legend>
          <label class="input input-sm flex w-full items-center gap-2">
            <span class="label shrink-0">+86</span>
            <input
              v-model="phone"
              class="min-w-0 grow"
              type="tel"
              inputmode="numeric"
              autocomplete="tel-national"
              maxlength="11"
              :disabled="Boolean(phoneLogin)"
              :placeholder="t('11-digit phone number')"
              :aria-label="t('KuGou phone number')"
            />
          </label>
        </fieldset>

        <div class="mt-3 flex flex-wrap items-center gap-2">
          <button
            class="btn btn-sm"
            type="button"
            :disabled="!isPhoneValid || isSendingCode || isCompletingPhoneLogin || resendSeconds > 0"
            @click="sendVerificationCode"
          >
            <RefreshCw v-if="isSendingCode" class="animate-spin" :size="16" aria-hidden="true" />
            <MessageSquareText v-else :size="16" aria-hidden="true" />
            <template v-if="isSendingCode">{{ t("Sending") }}</template>
            <template v-else-if="resendSeconds > 0">{{ t("Resend in {seconds}s", { seconds: resendSeconds }) }}</template>
            <template v-else-if="phoneLogin">{{ t("Resend code") }}</template>
            <template v-else>{{ t("Send code") }}</template>
          </button>
          <button v-if="phoneLogin" class="btn btn-ghost btn-sm" type="button" @click="changePhoneNumber">
            {{ t("Change phone number") }}
          </button>
        </div>

        <fieldset v-if="phoneLogin" class="fieldset mt-3">
          <legend class="fieldset-legend">{{ t("Verification code") }}</legend>
          <input
            v-model="verificationCode"
            class="input input-sm w-full"
            type="text"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="8"
            :placeholder="t('SMS verification code')"
            :aria-label="t('KuGou verification code')"
          />
        </fieldset>

        <div class="mt-5 flex flex-wrap gap-2">
          <button
            v-if="phoneLogin"
            class="btn btn-primary btn-sm"
            type="submit"
            :disabled="!isVerificationCodeValid || isCompletingPhoneLogin"
          >
            <RefreshCw v-if="isCompletingPhoneLogin" class="animate-spin" :size="16" aria-hidden="true" />
            <LogIn v-else :size="16" aria-hidden="true" />
            {{ isCompletingPhoneLogin ? t("Signing in") : t("Sign in") }}
          </button>
          <button class="btn btn-ghost btn-sm" type="button" @click="closeLogin">
            <X :size="16" aria-hidden="true" />
            {{ t("Cancel") }}
          </button>
        </div>
      </form>

      <div v-else-if="qrLogin" class="mt-5 grid gap-5 sm:grid-cols-[16rem_minmax(0,1fr)] sm:items-center">
        <img
          class="aspect-square w-full max-w-64 border border-base-300 bg-white p-2"
          :src="qrLogin.qrImageDataUrl"
          :alt="t('KuGou login QR code')"
        />
        <div class="min-w-0">
          <div class="flex items-center gap-2 text-sm font-medium">
            <RefreshCw v-if="isPollingQr" class="animate-spin" :size="16" aria-hidden="true" />
            <Clock3 v-else :size="16" aria-hidden="true" />
            {{ qrStatus }}
          </div>
          <p class="mt-2 text-sm text-muted">{{ t("Scan with the KuGou Music mobile app.") }}</p>
          <button class="btn btn-ghost btn-sm mt-4" type="button" @click="cancelQrLogin">
            <X :size="16" aria-hidden="true" />
            {{ t("Cancel QR code") }}
          </button>
        </div>
      </div>

      <div v-else class="mt-5 flex flex-wrap gap-2">
        <button class="btn btn-sm" type="button" :disabled="isConnectingQr" @click="startQrLogin">
          <RefreshCw v-if="isConnectingQr" class="animate-spin" :size="16" aria-hidden="true" />
          <QrCode v-else :size="16" aria-hidden="true" />
          {{ t("Create QR code") }}
        </button>
        <button class="btn btn-ghost btn-sm" type="button" @click="closeLogin">
          <X :size="16" aria-hidden="true" />
          {{ t("Cancel") }}
        </button>
      </div>
    </div>

    <div v-else-if="!activeAccountRef" class="grid min-h-52 place-items-center p-8 text-center">
      <div>
        <UserRound class="mx-auto text-base-content/35" :size="30" aria-hidden="true" />
        <div class="mt-3 text-sm font-medium">{{ t("No KuGou account connected") }}</div>
      </div>
    </div>
  </section>
</template>
