<script setup lang="ts">
import {
  Captions,
  Palette,
  RotateCcw,
  TextAlignCenter,
  TextAlignEnd,
  TextAlignStart,
  Type,
} from "@lucide/vue";
import {
  MIN_NOW_PLAYING_LYRICS_INACTIVE_OPACITY,
  NOW_PLAYING_LYRICS_FONT_OPTIONS,
  NOW_PLAYING_LYRICS_SETTINGS_ID,
  NOW_PLAYING_LYRICS_THEME_COLOR,
  nowPlayingLyricsColor,
  nowPlayingLyricsFontFamily,
  type NowPlayingLyricsAlignment,
  type NowPlayingLyricsPreferences,
} from "../lib/now-playing-lyrics";
import { t } from "../i18n";

type ColorPreference = "activeColor" | "inactiveColor";

const colorControls = [
  {
    preference: "activeColor",
    label: "Current line",
    inputLabel: "Current lyric color",
    themeLabel: "Use theme color for current lyric",
    fallback: "#111827",
  },
  {
    preference: "inactiveColor",
    label: "Other lines",
    inputLabel: "Other lyrics color",
    themeLabel: "Use theme color for other lyrics",
    fallback: "#6b7280",
  },
] as const satisfies ReadonlyArray<{
  preference: ColorPreference;
  label: string;
  inputLabel: string;
  themeLabel: string;
  fallback: string;
}>;

const props = defineProps<{
  preferences: NowPlayingLyricsPreferences;
}>();

const emit = defineEmits<{
  update: [patch: Partial<NowPlayingLyricsPreferences>];
  reset: [];
}>();

function update(patch: Partial<NowPlayingLyricsPreferences>) {
  emit("update", patch);
}

function numberValue(event: Event) {
  return Number((event.currentTarget as HTMLInputElement | HTMLSelectElement).value);
}

function stringValue(event: Event) {
  return (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
}

function setAlignment(alignment: NowPlayingLyricsAlignment) {
  update({ alignment });
}

function setColor(preference: ColorPreference, color: string) {
  update({ [preference]: color });
}

function colorPickerValue(color: string, fallback: string) {
  return color === NOW_PLAYING_LYRICS_THEME_COLOR ? fallback : color;
}

function previewLineStyle(active: boolean) {
  return {
    color: nowPlayingLyricsColor(
      active ? props.preferences.activeColor : props.preferences.inactiveColor,
    ),
    fontSize: `${props.preferences.fontSize}px`,
    fontWeight: active ? props.preferences.activeFontWeight : 400,
    lineHeight: `${Math.max(24, Math.round(props.preferences.fontSize * 1.4))}px`,
    opacity: active ? 1 : props.preferences.inactiveOpacity,
    paddingBlock: `${props.preferences.lineGap / 2}px`,
  };
}
</script>

<template>
  <section
    :id="NOW_PLAYING_LYRICS_SETTINGS_ID"
    class="overflow-hidden rounded border border-base-300 bg-base-100 focus:outline-none"
    tabindex="-1"
  >
    <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
      <Captions :size="18" aria-hidden="true" />
      <h2 class="min-w-0 flex-1 text-base font-semibold">{{ t("Now playing lyrics") }}</h2>
    </div>

    <div
      class="border-b border-base-300 bg-base-200 px-6 py-4"
      :aria-label="t('Now playing lyrics preview')"
    >
      <div
        class="mx-auto max-w-xl overflow-hidden"
        :style="{
          fontFamily: nowPlayingLyricsFontFamily(preferences.font),
          textAlign: preferences.alignment,
        }"
      >
        <p :style="previewLineStyle(false)">{{ t("Slow mornings, open windows") }}</p>
        <p :style="previewLineStyle(true)">{{ t("Coffee cools, the melody stays") }}</p>
        <p :style="previewLineStyle(false)">{{ t("Another quiet song begins") }}</p>
      </div>
    </div>

    <div class="divide-y divide-base-300">
      <div class="grid gap-4 px-4 py-4 sm:grid-cols-2">
        <label class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-3 text-sm font-medium">
            <Type class="text-muted" :size="17" aria-hidden="true" />
            {{ t("Typeface") }}
          </span>
          <select
            class="select select-sm w-40"
            :value="preferences.font"
            :aria-label="t('Now playing lyric typeface')"
            @change="update({ font: stringValue($event) as NowPlayingLyricsPreferences['font'] })"
          >
            <option
              v-for="font in NOW_PLAYING_LYRICS_FONT_OPTIONS"
              :key="font.value"
              :value="font.value"
            >
              {{ t(font.label) }}
            </option>
          </select>
        </label>

        <label class="flex items-center justify-between gap-3 text-sm font-medium">
          {{ t("Current line weight") }}
          <select
            class="select select-sm w-40"
            :value="preferences.activeFontWeight"
            :aria-label="t('Current lyric weight')"
            @change="update({ activeFontWeight: numberValue($event) as NowPlayingLyricsPreferences['activeFontWeight'] })"
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
            <output class="text-xs tabular-nums text-muted">
              {{ preferences.fontSize }} px
            </output>
          </span>
          <input
            class="range range-sm min-h-6 w-full"
            type="range"
            min="12"
            max="30"
            step="1"
            :value="preferences.fontSize"
            :aria-label="t('Now playing lyric size')"
            @input="update({ fontSize: numberValue($event) })"
          />
        </label>

        <label class="min-w-0">
          <span class="mb-2 flex items-center justify-between gap-3 text-sm font-medium">
            {{ t("Line spacing") }}
            <output class="text-xs tabular-nums text-muted">
              {{ preferences.lineGap }} px
            </output>
          </span>
          <input
            class="range range-sm min-h-6 w-full"
            type="range"
            min="4"
            max="28"
            step="2"
            :value="preferences.lineGap"
            :aria-label="t('Now playing lyric line spacing')"
            @input="update({ lineGap: numberValue($event) })"
          />
        </label>
      </div>

      <div class="grid gap-5 px-4 py-4 sm:grid-cols-2">
        <div class="flex items-center justify-between gap-3">
          <span class="text-sm font-medium">{{ t("Alignment") }}</span>
          <div class="join" :aria-label="t('Now playing lyrics alignment')">
            <button
              class="btn btn-square btn-sm join-item"
              :class="{ 'btn-active': preferences.alignment === 'left' }"
              type="button"
              :aria-label="t('Align now playing lyrics left')"
              :aria-pressed="preferences.alignment === 'left'"
              :title="t('Left')"
              @click="setAlignment('left')"
            >
              <TextAlignStart :size="16" aria-hidden="true" />
            </button>
            <button
              class="btn btn-square btn-sm join-item"
              :class="{ 'btn-active': preferences.alignment === 'center' }"
              type="button"
              :aria-label="t('Center now playing lyrics')"
              :aria-pressed="preferences.alignment === 'center'"
              :title="t('Center')"
              @click="setAlignment('center')"
            >
              <TextAlignCenter :size="16" aria-hidden="true" />
            </button>
            <button
              class="btn btn-square btn-sm join-item"
              :class="{ 'btn-active': preferences.alignment === 'right' }"
              type="button"
              :aria-label="t('Align now playing lyrics right')"
              :aria-pressed="preferences.alignment === 'right'"
              :title="t('Right')"
              @click="setAlignment('right')"
            >
              <TextAlignEnd :size="16" aria-hidden="true" />
            </button>
          </div>
        </div>

        <label class="min-w-0">
          <span class="mb-2 flex items-center justify-between gap-3 text-sm font-medium">
            {{ t("Other line opacity") }}
            <output class="text-xs tabular-nums text-muted">
              {{ Math.round(preferences.inactiveOpacity * 100) }}%
            </output>
          </span>
          <input
            class="range range-sm min-h-6 w-full"
            type="range"
            :min="MIN_NOW_PLAYING_LYRICS_INACTIVE_OPACITY"
            max="1"
            step="0.05"
            :value="preferences.inactiveOpacity"
            :aria-label="t('Other lyric opacity')"
            @input="update({ inactiveOpacity: numberValue($event) })"
          />
        </label>
      </div>

      <div class="grid gap-4 px-4 py-4 sm:grid-cols-2">
        <div
          v-for="control in colorControls"
          :key="control.preference"
          class="flex items-center justify-between gap-3 text-sm font-medium"
        >
          <span class="flex items-center gap-3">
            <Palette class="text-muted" :size="17" aria-hidden="true" />
            {{ t(control.label) }}
          </span>
          <div class="flex shrink-0 items-center gap-1">
            <input
              class="size-9 cursor-pointer rounded border border-base-300 bg-transparent p-1"
              type="color"
              :value="colorPickerValue(preferences[control.preference], control.fallback)"
              :aria-label="t(control.inputLabel)"
              @input="setColor(control.preference, stringValue($event))"
            />
            <div class="tooltip tooltip-bottom" :data-tip="t('Use theme color')">
              <button
                class="btn btn-square btn-ghost btn-sm"
                :class="{
                  'btn-active': preferences[control.preference] === NOW_PLAYING_LYRICS_THEME_COLOR,
                }"
                type="button"
                :aria-label="t(control.themeLabel)"
                :aria-pressed="preferences[control.preference] === NOW_PLAYING_LYRICS_THEME_COLOR"
                :title="t('Use theme color')"
                @click="setColor(control.preference, NOW_PLAYING_LYRICS_THEME_COLOR)"
              >
                <Palette :size="16" aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="flex justify-end px-4 py-3">
        <button class="btn btn-ghost btn-sm" type="button" @click="emit('reset')">
          <RotateCcw :size="16" aria-hidden="true" />
          {{ t("Reset now playing lyrics") }}
        </button>
      </div>
    </div>
  </section>
</template>
