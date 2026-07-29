<script setup lang="ts">
import {
  Ban,
  Captions,
  Lock,
  MonitorUp,
  MousePointer2Off,
  PanelTop,
  RotateCcw,
  TextAlignCenter,
  TextAlignEnd,
  TextAlignStart,
  Type,
} from "@lucide/vue";
import DesktopLyricTextEffect from "./DesktopLyricTextEffect.vue";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_FONT_OPTIONS,
  DESKTOP_LYRICS_TRANSPARENT_COLOR,
  type DesktopLyricsAlignment,
  type DesktopLyricsPreferences,
} from "../lib/desktop-lyrics";
import { t } from "../i18n";

type DesktopLyricsColorPreference = "activeColor" | "inactiveColor" | "backgroundColor";

const colorControls = [
  {
    preference: "activeColor",
    label: "Current line",
    inputLabel: "Current lyric color",
    noColorLabel: "Use no color for current lyric",
    fallback: DEFAULT_DESKTOP_LYRICS_PREFERENCES.activeColor,
  },
  {
    preference: "inactiveColor",
    label: "Next line",
    inputLabel: "Next lyric color",
    noColorLabel: "Use no color for next lyric",
    fallback: DEFAULT_DESKTOP_LYRICS_PREFERENCES.inactiveColor,
  },
  {
    preference: "backgroundColor",
    label: "Background",
    inputLabel: "Desktop lyrics background color",
    noColorLabel: "Use no color for desktop lyrics background",
    fallback: "#111827",
  },
] as const satisfies ReadonlyArray<{
  preference: DesktopLyricsColorPreference;
  label: string;
  inputLabel: string;
  noColorLabel: string;
  fallback: string;
}>;

const props = defineProps<{
  preferences: DesktopLyricsPreferences;
}>();

const emit = defineEmits<{
  update: [patch: Partial<DesktopLyricsPreferences>];
  reset: [];
}>();

function update(patch: Partial<DesktopLyricsPreferences>) {
  emit("update", patch);
}

function checkboxValue(event: Event) {
  return (event.currentTarget as HTMLInputElement).checked;
}

function numberValue(event: Event) {
  return Number((event.currentTarget as HTMLInputElement).value);
}

function stringValue(event: Event) {
  return (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
}

function setAlignment(alignment: DesktopLyricsAlignment) {
  update({ alignment });
}

function setColor(preference: DesktopLyricsColorPreference, color: string) {
  update({ [preference]: color });
}

function colorPickerValue(color: string, fallback: string) {
  return color === DESKTOP_LYRICS_TRANSPARENT_COLOR ? fallback : color;
}
</script>

<template>
  <section class="overflow-hidden rounded border border-base-300 bg-base-100">
    <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
      <Captions :size="18" aria-hidden="true" />
      <h2 class="min-w-0 flex-1 text-base font-semibold">{{ t("Lyrics display") }}</h2>
    </div>

    <div class="border-b border-base-300 bg-neutral p-5 text-neutral-content">
      <div
        class="mx-auto flex min-h-28 max-w-3xl flex-col justify-center overflow-hidden rounded border border-white/15 px-5 py-4"
        :style="{
          backgroundColor: `color-mix(in srgb, ${preferences.backgroundColor} ${Math.round(preferences.backgroundOpacity * 100)}%, transparent)`,
          fontFamily: preferences.font === 'serif' ? 'serif' : preferences.font === 'monospace' ? 'monospace' : 'sans-serif',
          textAlign: preferences.alignment,
        }"
        :aria-label="t('Desktop lyrics preview')"
      >
        <div
          class="leading-tight"
          :style="{
            fontSize: `${Math.min(preferences.fontSize, 44)}px`,
            fontWeight: preferences.fontWeight,
          }"
        >
          <DesktopLyricTextEffect
            :effect="preferences.effect"
            :text="t('Coffee cools, the melody stays')"
            :text-color="preferences.activeColor"
            :lines="2"
            data-testid="desktop-lyrics-preview-effect"
          >
            <span
              class="desktop-lyric-preview-fill bg-clip-text text-transparent"
              :style="{
                backgroundImage: `linear-gradient(90deg, ${preferences.activeColor} 0%, ${preferences.activeColor} 52%, ${preferences.inactiveColor} 52%, ${preferences.inactiveColor} 100%)`,
              }"
            >{{ t("Coffee cools, the melody stays") }}</span>
          </DesktopLyricTextEffect>
        </div>
        <div
          v-if="preferences.showNextLine"
          class="mt-2 truncate text-base leading-tight"
          :style="{ color: preferences.inactiveColor }"
        >
          <DesktopLyricTextEffect
            :effect="preferences.effect"
            :text="t('Another quiet song begins')"
            :text-color="preferences.inactiveColor"
            :lines="1"
          />
        </div>
      </div>
    </div>

    <div class="divide-y divide-base-300">
      <div class="grid gap-4 px-4 py-4 sm:grid-cols-2">
        <label class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <Captions class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Desktop overlay") }}
          </span>
          <input
            class="toggle toggle-md"
            type="checkbox"
            :checked="preferences.enabled"
            :aria-label="t('Show desktop lyrics')"
            @change="update({ enabled: checkboxValue($event) })"
          />
        </label>

        <label class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <PanelTop class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Menu bar lyrics") }} <span class="text-xs font-normal text-muted">macOS</span>
          </span>
          <input
            class="toggle toggle-md"
            type="checkbox"
            :checked="preferences.menuBarEnabled"
            :aria-label="t('Show lyrics in macOS menu bar')"
            @change="update({ menuBarEnabled: checkboxValue($event) })"
          />
        </label>

        <label class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <MonitorUp class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Always on top") }}
          </span>
          <input
            class="toggle toggle-md"
            type="checkbox"
            :checked="preferences.alwaysOnTop"
            @change="update({ alwaysOnTop: checkboxValue($event) })"
          />
        </label>

        <label class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <Lock class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Lock window") }}
          </span>
          <input
            class="toggle toggle-md"
            type="checkbox"
            :checked="preferences.locked"
            @change="update({ locked: checkboxValue($event) })"
          />
        </label>

        <label class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <Captions class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Next line") }}
          </span>
          <input
            class="toggle toggle-md"
            type="checkbox"
            :checked="preferences.showNextLine"
            @change="update({ showNextLine: checkboxValue($event) })"
          />
        </label>

        <div class="flex items-center justify-between gap-3">
          <span class="flex min-w-0 items-center gap-3 text-sm font-medium">
            <MousePointer2Off class="shrink-0 text-muted" :size="17" aria-hidden="true" />
            {{ t("Pointer passthrough") }}
          </span>
          <span class="text-xs text-muted">{{ preferences.locked ? t("On") : t("Off") }}</span>
        </div>
      </div>

      <div class="grid gap-4 px-4 py-4 sm:grid-cols-3">
        <div
          v-for="control in colorControls"
          :key="control.preference"
          class="flex items-center justify-between gap-3 text-sm font-medium"
        >
          <span>{{ t(control.label) }}</span>
          <div class="flex shrink-0 items-center gap-1">
            <input
              class="size-9 cursor-pointer rounded border border-base-300 bg-transparent p-1"
              type="color"
              :value="colorPickerValue(preferences[control.preference], control.fallback)"
              :aria-label="t(control.inputLabel)"
              @input="setColor(control.preference, stringValue($event))"
            />
            <div class="tooltip tooltip-bottom" :data-tip="t('No color')">
              <button
                class="btn btn-square btn-ghost btn-sm"
                :class="{
                  'btn-active': preferences[control.preference] === DESKTOP_LYRICS_TRANSPARENT_COLOR,
                }"
                type="button"
                :aria-label="t(control.noColorLabel)"
                :aria-pressed="preferences[control.preference] === DESKTOP_LYRICS_TRANSPARENT_COLOR"
                :title="t('No color')"
                @click="setColor(control.preference, DESKTOP_LYRICS_TRANSPARENT_COLOR)"
              >
                <Ban :size="16" aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <label
        v-if="preferences.menuBarEnabled"
        class="flex items-center justify-between gap-3 px-4 py-4"
      >
        <span class="text-sm font-medium">{{ t("Menu bar width") }}</span>
        <select
          class="select select-sm w-36"
          :value="preferences.menuBarMaxWidth"
          :aria-label="t('Menu bar lyric width')"
          @change="update({ menuBarMaxWidth: Number(stringValue($event)) })"
        >
          <option :value="24">{{ t("Compact") }}</option>
          <option :value="40">{{ t("Standard") }}</option>
          <option :value="56">{{ t("Wide") }}</option>
        </select>
      </label>

      <div class="grid gap-4 px-4 py-4 sm:grid-cols-2">
        <label class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-3 text-sm font-medium">
            <Type class="text-muted" :size="17" aria-hidden="true" />
            {{ t("Typeface") }}
          </span>
          <select
            class="select select-sm w-40"
            :value="preferences.font"
            @change="update({ font: stringValue($event) as DesktopLyricsPreferences['font'] })"
          >
            <option v-for="font in DESKTOP_LYRICS_FONT_OPTIONS" :key="font.value" :value="font.value">
              {{ t(font.label) }}
            </option>
          </select>
        </label>

        <label class="flex items-center justify-between gap-3 text-sm font-medium">
          {{ t("Weight") }}
          <select
            class="select select-sm w-40"
            :value="preferences.fontWeight"
            @change="update({ fontWeight: Number(stringValue($event)) })"
          >
            <option :value="400">{{ t("Regular") }}</option>
            <option :value="500">{{ t("Medium") }}</option>
            <option :value="600">{{ t("Semibold") }}</option>
            <option :value="700">{{ t("Bold") }}</option>
            <option :value="800">{{ t("Extra bold") }}</option>
          </select>
        </label>
      </div>

      <div class="grid gap-5 px-4 py-4 sm:grid-cols-2">
        <label class="min-w-0">
          <span class="mb-2 flex items-center justify-between gap-3 text-sm font-medium">
            {{ t("Size") }}
            <output class="text-xs tabular-nums text-muted">{{ preferences.fontSize }} px</output>
          </span>
          <input
            class="range range-sm min-h-6 w-full"
            type="range"
            min="18"
            max="72"
            step="1"
            :value="preferences.fontSize"
            @input="update({ fontSize: numberValue($event) })"
          />
        </label>

        <label class="min-w-0">
          <span class="mb-2 flex items-center justify-between gap-3 text-sm font-medium">
            {{ t("Background opacity") }}
            <output class="text-xs tabular-nums text-muted">
              {{ Math.round(preferences.backgroundOpacity * 100) }}%
            </output>
          </span>
          <input
            class="range range-sm min-h-6 w-full"
            type="range"
            min="0"
            max="1"
            step="0.01"
            :value="preferences.backgroundOpacity"
            @input="update({ backgroundOpacity: numberValue($event) })"
          />
        </label>
      </div>

      <div class="flex flex-col gap-4 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
        <div class="flex items-center gap-3">
          <span class="text-sm font-medium">{{ t("Alignment") }}</span>
          <div class="join" :aria-label="t('Desktop lyrics alignment')">
            <div class="tooltip" :data-tip="t('Left')">
              <button
                class="btn btn-square btn-sm join-item"
                :class="{ 'btn-active': preferences.alignment === 'left' }"
                type="button"
                :aria-label="t('Align desktop lyrics left')"
                :aria-pressed="preferences.alignment === 'left'"
                @click="setAlignment('left')"
              >
                <TextAlignStart :size="16" aria-hidden="true" />
              </button>
            </div>
            <div class="tooltip" :data-tip="t('Center')">
              <button
                class="btn btn-square btn-sm join-item"
                :class="{ 'btn-active': preferences.alignment === 'center' }"
                type="button"
                :aria-label="t('Center desktop lyrics')"
                :aria-pressed="preferences.alignment === 'center'"
                @click="setAlignment('center')"
              >
                <TextAlignCenter :size="16" aria-hidden="true" />
              </button>
            </div>
            <div class="tooltip" :data-tip="t('Right')">
              <button
                class="btn btn-square btn-sm join-item"
                :class="{ 'btn-active': preferences.alignment === 'right' }"
                type="button"
                :aria-label="t('Align desktop lyrics right')"
                :aria-pressed="preferences.alignment === 'right'"
                @click="setAlignment('right')"
              >
                <TextAlignEnd :size="16" aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>

        <label class="flex items-center justify-between gap-3 sm:justify-start">
          <span class="text-sm font-medium">{{ t("Text effect") }}</span>
          <select
            class="select select-sm w-40"
            :value="preferences.effect"
            @change="update({ effect: stringValue($event) as DesktopLyricsPreferences['effect'] })"
          >
            <option value="shadow">{{ t("Shadow") }}</option>
            <option value="outline">{{ t("Outline") }}</option>
            <option value="none">{{ t("None") }}</option>
          </select>
        </label>
      </div>

      <div class="flex justify-end px-4 py-3">
        <button class="btn btn-ghost btn-sm" type="button" @click="emit('reset')">
          <RotateCcw :size="16" aria-hidden="true" />
          {{ t("Reset lyrics display") }}
        </button>
      </div>
    </div>
  </section>
</template>
