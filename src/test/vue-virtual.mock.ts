import { ref } from "vue";
import { vi } from "vitest";

type VirtualizerOptions = {
  value: {
    count: number;
    estimateSize: () => number;
  };
};

export function useVirtualizer(options: VirtualizerOptions) {
  return ref({
    getVirtualItems: () =>
      Array.from({ length: Math.min(options.value.count, 20) }, (_, index) => ({
        index,
        key: index,
        start: index * options.value.estimateSize(),
        size: options.value.estimateSize(),
        end: (index + 1) * options.value.estimateSize(),
        lane: 0,
      })),
    getTotalSize: () => options.value.count * options.value.estimateSize(),
    measure: vi.fn(),
    scrollToIndex: vi.fn(),
  });
}
