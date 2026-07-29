<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useMutation, useQuery } from "@tanstack/vue-query";
import {
  AlertCircle,
  CircleCheck,
  Clock3,
  ListMusic,
  LogOut,
  Power,
  QrCode,
  RefreshCw,
  UserRound,
  X,
} from "@lucide/vue";
import { listPlugins } from "../lib/plugin-api";
import { useQrLoginSession } from "../composables/source-workspace";
import { normalizeError, queryError } from "../lib/errors";
import { t } from "../i18n";
import type {
  AudioSourceId,
  AudioSourceOption,
} from "../lib/audio-source-api";
import {
  NETEASE_PLUGIN_ID,
  cancelNeteaseQrLogin,
  disconnectNeteaseAccount,
  listNeteaseAccounts,
  pollNeteaseQrLogin,
  startNeteaseQrLogin,
} from "../lib/netease-api";

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

const pluginsQuery = useQuery({
  queryKey: ["plugins"],
  queryFn: listPlugins,
  staleTime: 0,
});

const accountsQuery = useQuery({
  queryKey: ["netease", "accounts"],
  queryFn: listNeteaseAccounts,
  staleTime: 0,
});

const disconnectMutation = useMutation({
  mutationFn: (accountRef: string) => disconnectNeteaseAccount(accountRef),
});

const plugin = computed(
  () => pluginsQuery.data.value?.find((record) => record.id === NETEASE_PLUGIN_ID) ?? null,
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
  providerName: "NetEase Cloud Music",
  start: startNeteaseQrLogin,
  poll: pollNeteaseQrLogin,
  cancel: cancelNeteaseQrLogin,
  onConnected: connectAccount,
  onError: (error) => {
    sourceError.value = normalizeError(error);
  },
});
const qrLogin = qrSession.login;
const qrStatus = qrSession.status;
const isConnecting = qrSession.isConnecting;
const isPollingQr = qrSession.isPolling;
const cancelQrLogin = qrSession.cancel;

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
});

async function startQrLogin() {
  if (!isPluginReady.value || isConnecting.value) {
    return;
  }
  sourceError.value = null;
  sourceNotice.value = null;
  await qrSession.start();
}

async function connectAccount(account: { accountRef: string; displayName: string }) {
  await accountsQuery.refetch();
  activeAccountRef.value = account.accountRef;
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
            <h2 class="text-base font-semibold">NetEase Cloud Music</h2>
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
          :aria-label="t('NetEase playback source')"
          :title="t('Playback source')"
          @change="selectPlaybackSource"
        >
          <option v-if="!audioSources.length" value="">{{ t("No audio source") }}</option>
          <option
            v-for="source in audioSources"
            :key="source.value"
            :value="source.value"
          >
            {{ source.label }}
          </option>
        </select>
        <select
          v-if="accounts.length"
          v-model="activeAccountRef"
          class="select select-sm w-32 max-w-full sm:w-44"
          :aria-label="t('NetEase account')"
        >
          <option v-for="account in accounts" :key="account.accountRef" :value="account.accountRef">
            {{ account.displayName }}{{ account.status === "expired" ? t(" (expired)") : "" }}
          </option>
        </select>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!isPluginReady || isConnecting"
          @click="startQrLogin"
        >
          <RefreshCw v-if="isConnecting" class="animate-spin" :size="16" aria-hidden="true" />
          <QrCode v-else :size="16" aria-hidden="true" />
          {{ t("Connect") }}
        </button>
        <button
          v-if="activeAccount"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="t('Disconnect NetEase account')"
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
      <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Dismiss NetEase error')" @click="sourceError = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="sourceNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceNotice }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Dismiss NetEase notice')" @click="sourceNotice = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div
      v-if="isPluginReady && !audioSources.length"
      role="status"
      class="alert alert-warning alert-soft m-4"
    >
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ t("No enabled audio source is available.") }}</span>
      <button class="btn btn-sm" type="button" @click="emit('openAudioSources')">
        {{ t("Open Audio Sources") }}
      </button>
    </div>

    <div v-if="!isPluginReady" class="flex flex-col gap-3 p-5 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex items-start gap-3">
        <Power class="mt-0.5 shrink-0 text-warning" :size="18" aria-hidden="true" />
        <div>
          <div class="text-sm font-medium">{{ t("Plugin is disabled") }}</div>
          <div class="mt-1 text-xs text-muted">{{ t("Enable the bundled Plugin to use NetEase.") }}</div>
        </div>
      </div>
      <button class="btn btn-sm" type="button" @click="emit('openPlugins')">{{ t("Open Plugins") }}</button>
    </div>

    <div v-else-if="qrLogin" class="grid gap-5 p-5 sm:grid-cols-[16rem_minmax(0,1fr)] sm:items-center">
      <img
        class="aspect-square w-full max-w-64 border border-base-300 bg-white p-2"
        :src="qrLogin.qrImageDataUrl"
        :alt="t('NetEase login QR code')"
      />
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-sm font-medium">
          <RefreshCw v-if="isPollingQr" class="animate-spin" :size="16" aria-hidden="true" />
          <Clock3 v-else :size="16" aria-hidden="true" />
          {{ qrStatus }}
        </div>
        <p class="mt-2 text-sm text-muted">{{ t("Scan with the NetEase Cloud Music mobile app.") }}</p>
        <button class="btn btn-ghost btn-sm mt-4" type="button" @click="cancelQrLogin">
          <X :size="16" aria-hidden="true" />
          {{ t("Cancel") }}
        </button>
      </div>
    </div>

    <div v-else-if="!activeAccountRef" class="grid min-h-52 place-items-center p-8 text-center">
      <div>
        <UserRound class="mx-auto text-base-content/35" :size="30" aria-hidden="true" />
        <div class="mt-3 text-sm font-medium">{{ t("No NetEase account connected") }}</div>
      </div>
    </div>
  </section>
</template>
