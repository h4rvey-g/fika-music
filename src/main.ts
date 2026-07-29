import { createApp } from "vue";
import { QueryClient, VueQueryPlugin } from "@tanstack/vue-query";
import App from "./App.vue";
import DesktopLyricsWindow from "./components/DesktopLyricsWindow.vue";
import { setLocale } from "./i18n";
import { loadUiPreferences } from "./lib/ui-preferences";
import "./style.css";

setLocale(loadUiPreferences().locale);

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

const isDesktopLyricsWindow =
  new URLSearchParams(window.location.search).get("window") === "desktop-lyrics";
const rootComponent = isDesktopLyricsWindow ? DesktopLyricsWindow : App;

createApp(rootComponent).use(VueQueryPlugin, { queryClient }).mount("#app");
