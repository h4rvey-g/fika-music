import { createApp } from "vue";
import { QueryClient, VueQueryPlugin } from "@tanstack/vue-query";
import App from "./App.vue";
import "./style.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      staleTime: 15_000,
    },
    mutations: {
      retry: false,
    },
  },
});

createApp(App).use(VueQueryPlugin, { queryClient }).mount("#app");
