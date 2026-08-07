<script setup lang="ts">
import { computed } from "vue";
import {
  AlertCircle,
  Check,
  Download,
  RefreshCw,
  RotateCcw,
} from "@lucide/vue";
import { currentLocale, t } from "../i18n";
import type { AppUpdateSummary } from "../composables/use-app-updater";

const props = defineProps<{
  currentVersion: string | null;
  update: AppUpdateSummary | null;
  isChecking: boolean;
  isInstalling: boolean;
  hasChecked: boolean;
  restartRequired: boolean;
  downloadedBytes: number;
  totalBytes: number | null;
  downloadPercent: number;
  error: string | null;
}>();

const emit = defineEmits<{
  check: [];
  install: [];
  restart: [];
}>();

const releaseDate = computed(() => {
  if (!props.update?.date) return null;
  const date = new Date(props.update.date);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(currentLocale.value, { dateStyle: "medium" }).format(date);
});

const downloadMessage = computed(() => {
  if (!props.totalBytes) return t("Downloading update");
  return t("{downloaded} of {total}", {
    downloaded: formatBytes(props.downloadedBytes),
    total: formatBytes(props.totalBytes),
  });
});

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const unitIndex = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / (1024 ** unitIndex);
  const maximumFractionDigits = unitIndex === 0 || value >= 10 ? 0 : 1;
  return `${new Intl.NumberFormat(currentLocale.value, { maximumFractionDigits }).format(value)} ${units[unitIndex]}`;
}
</script>

<template>
  <section
    data-testid="app-update-settings"
    class="overflow-hidden rounded border border-base-300 bg-base-100"
  >
    <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
      <RefreshCw :size="18" aria-hidden="true" />
      <h2 class="text-base font-semibold">{{ t("Software update") }}</h2>
    </div>

    <div class="divide-y divide-base-300">
      <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
        <div class="min-w-0">
          <div class="text-sm font-medium">{{ t("Current version") }}</div>
          <div class="text-xs tabular-nums text-muted">
            {{ currentVersion ? `v${currentVersion}` : t("Unavailable") }}
          </div>
        </div>

        <div class="flex min-h-8 items-center gap-2 text-sm" role="status" aria-live="polite">
          <RefreshCw v-if="isChecking" class="animate-spin" :size="16" aria-hidden="true" />
          <Download v-else-if="update" :size="16" aria-hidden="true" />
          <Check v-else-if="hasChecked" class="text-success" :size="17" aria-hidden="true" />
          <span v-if="isChecking">{{ t("Checking for updates") }}</span>
          <span v-else-if="update">{{ t("Version {version} is available", { version: update.version }) }}</span>
          <span v-else-if="hasChecked">{{ t("Up to date") }}</span>
        </div>
      </div>

      <div v-if="error || update" class="flex flex-col gap-4 px-4 py-4">
        <div v-if="error" role="alert" class="alert alert-error alert-soft">
          <AlertCircle :size="18" aria-hidden="true" />
          <span class="min-w-0">{{ t(error) }}</span>
        </div>

        <template v-if="update">
          <div class="min-w-0">
            <div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
              <h3 class="text-sm font-semibold">
                {{ t("Version {version}", { version: update.version }) }}
              </h3>
              <span v-if="releaseDate" class="text-xs text-muted">
                {{ t("Published {date}", { date: releaseDate }) }}
              </span>
            </div>
            <p v-if="update.body" class="mt-2 max-h-32 overflow-y-auto whitespace-pre-wrap text-sm text-muted">
              {{ update.body }}
            </p>
          </div>

          <div v-if="isInstalling" class="flex flex-col gap-2" aria-live="polite">
            <div class="flex items-center justify-between gap-3 text-xs text-muted">
              <span>{{ downloadMessage }}</span>
              <span class="tabular-nums">{{ downloadPercent }}%</span>
            </div>
            <progress
              class="progress progress-primary h-2 w-full"
              :value="downloadPercent"
              max="100"
              :aria-label="t('Downloading update')"
            ></progress>
          </div>

          <div v-if="restartRequired" role="alert" class="alert alert-success alert-soft">
            <Check :size="18" aria-hidden="true" />
            <span>{{ t("Update installed. Restart Fika Music to finish.") }}</span>
          </div>

          <div class="flex justify-end">
            <button
              v-if="restartRequired"
              class="btn btn-primary btn-sm"
              type="button"
              @click="emit('restart')"
            >
              <RotateCcw :size="16" aria-hidden="true" />
              {{ t("Restart now") }}
            </button>
            <button
              v-else
              class="btn btn-primary btn-sm"
              type="button"
              :disabled="isChecking || isInstalling"
              @click="emit('install')"
            >
              <RefreshCw v-if="isInstalling" class="animate-spin" :size="16" aria-hidden="true" />
              <Download v-else :size="16" aria-hidden="true" />
              {{ isInstalling ? t("Installing update") : t("Download and install") }}
            </button>
          </div>
        </template>
      </div>

      <div v-if="!update && !restartRequired" class="flex justify-end px-4 py-4">
        <button
          class="btn btn-sm"
          type="button"
          :disabled="isChecking || isInstalling"
          @click="emit('check')"
        >
          <RefreshCw :class="{ 'animate-spin': isChecking }" :size="16" aria-hidden="true" />
          {{ t("Check for updates") }}
        </button>
      </div>
    </div>
  </section>
</template>
