import { QueryClient, VueQueryPlugin } from "@tanstack/vue-query";

export function createTestQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { retry: false },
    },
  });
}

export function createTestQueryPlugin(): [
  typeof VueQueryPlugin,
  { queryClient: QueryClient },
] {
  return [VueQueryPlugin, { queryClient: createTestQueryClient() }];
}
