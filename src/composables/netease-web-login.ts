import { ref, shallowRef } from "vue";
import {
  cancelNeteaseWebLogin,
  pollNeteaseWebLogin,
  startNeteaseWebLogin,
  type NeteaseAccount,
  type NeteaseWebLoginStart,
} from "../lib/netease-api";
import { t } from "../i18n";

export function useNeteaseWebLogin(options: {
  onConnected: (account: NeteaseAccount) => Promise<void>;
  onError: (error: unknown) => void;
}) {
  const login = shallowRef<NeteaseWebLoginStart | null>(null);
  const isStarting = ref(false);
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function cancel() {
    generation += 1;
    isStarting.value = false;
    if (timer) clearTimeout(timer);
    timer = null;
    const sessionId = login.value?.sessionId;
    login.value = null;
    if (sessionId) void cancelNeteaseWebLogin(sessionId).catch(() => undefined);
  }

  async function start() {
    if (isStarting.value || login.value) return;
    cancel();
    const requestGeneration = generation;
    isStarting.value = true;
    try {
      const session = await startNeteaseWebLogin();
      if (requestGeneration !== generation) {
        void cancelNeteaseWebLogin(session.sessionId).catch(() => undefined);
        return;
      }
      login.value = session;
      schedulePoll(requestGeneration);
    } catch (error) {
      if (requestGeneration === generation) options.onError(error);
    } finally {
      if (requestGeneration === generation) isStarting.value = false;
    }
  }

  function schedulePoll(requestGeneration: number) {
    timer = setTimeout(() => void poll(requestGeneration), 1_000);
  }

  async function poll(requestGeneration: number) {
    timer = null;
    const sessionId = login.value?.sessionId;
    if (!sessionId || requestGeneration !== generation) return;
    try {
      const result = await pollNeteaseWebLogin(sessionId);
      if (requestGeneration !== generation) return;
      if (result.status === "waiting") {
        schedulePoll(requestGeneration);
        return;
      }
      cancel();
      if (result.status === "connected" && result.account) {
        try {
          await options.onConnected(result.account);
        } catch (error) {
          options.onError(error);
        }
      } else if (result.status === "expired") {
        options.onError(new Error(t("Official website login expired. Sign in again.")));
      } else if (result.status !== "cancelled") {
        options.onError(new Error(t("Official website login completed without an account.")));
      }
    } catch (error) {
      if (requestGeneration === generation) {
        cancel();
        options.onError(error);
      }
    }
  }

  return { login, isStarting, start, cancel };
}
