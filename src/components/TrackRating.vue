<script setup lang="ts">
import { t } from "../i18n";

defineProps<{
  rating: number;
  name: string;
  title: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  change: [rating: number];
}>();

function changeRating(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  emit("change", Number(input.value));
}
</script>

<template>
  <div
    class="rating rating-xs shrink-0"
    role="radiogroup"
    :aria-label="t('Rating for {title}', { title })"
    @click.stop
    @dblclick.stop
  >
    <input
      class="rating-hidden"
      type="radio"
      :name="name"
      value="0"
      :checked="rating === 0"
      :disabled="disabled"
      :aria-label="t('Clear rating')"
      @change="changeRating"
    />
    <input
      v-for="value in 5"
      :key="value"
      class="mask mask-star bg-warning disabled:cursor-not-allowed"
      type="radio"
      :name="name"
      :value="value"
      :checked="rating === value"
      :disabled="disabled"
      :aria-label="t('{count} stars', { count: value })"
      @change="changeRating"
    />
  </div>
</template>
