<script setup lang="ts">
import { computed } from "vue";
import {
  desktopLyricsOutlineColor,
  type DesktopLyricsEffect,
} from "../lib/desktop-lyrics";

const props = defineProps<{
  effect: DesktopLyricsEffect;
  text: string;
  textColor: string;
  lines?: 1 | 2;
}>();

const outlineColor = computed(() => desktopLyricsOutlineColor(props.textColor));
</script>

<template>
  <span
    class="desktop-lyric-text-effect"
    :class="`desktop-lyric-text-effect-${effect}`"
    :data-text-effect="effect"
    :style="{ '--desktop-lyric-outline-color': outlineColor }"
  >
    <span
      v-if="effect === 'outline'"
      class="desktop-lyric-text-outline"
      :class="lines === 1 ? 'desktop-lyric-text-line-1' : 'desktop-lyric-text-line-2'"
      aria-hidden="true"
    >{{ text }}</span>
    <span
      class="desktop-lyric-text-fill"
      :class="lines === 1 ? 'desktop-lyric-text-line-1' : 'desktop-lyric-text-line-2'"
    >
      <slot>{{ text }}</slot>
    </span>
  </span>
</template>

<style scoped>
.desktop-lyric-text-effect,
.desktop-lyric-text-fill,
.desktop-lyric-text-outline {
  display: block;
  overflow-wrap: inherit;
  white-space: pre-wrap;
}

.desktop-lyric-text-effect {
  position: relative;
  isolation: isolate;
  font-kerning: none;
  font-variant-ligatures: none;
}

.desktop-lyric-text-fill {
  position: relative;
  z-index: 1;
}

.desktop-lyric-text-effect-shadow {
  filter:
    drop-shadow(0 1px 1px rgb(0 0 0 / 68%))
    drop-shadow(0 0.06em 0.08em rgb(0 0 0 / 34%));
}

.desktop-lyric-text-outline {
  position: absolute;
  z-index: 0;
  inset: 0;
  color: transparent;
  -webkit-text-stroke: clamp(1.5px, 0.055em, 2.5px) var(--desktop-lyric-outline-color);
  paint-order: stroke fill;
  pointer-events: none;
}

.desktop-lyric-text-line-1,
.desktop-lyric-text-line-2 {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
}

.desktop-lyric-text-line-1 {
  -webkit-line-clamp: 1;
}

.desktop-lyric-text-line-2 {
  -webkit-line-clamp: 2;
}
</style>
