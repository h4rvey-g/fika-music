import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";

export const CHKSZ_AUDIO_SOURCE_ID = "fika.chksz-netease-playback";

export function getChkszApiKeyStatus() {
  return invoke<boolean>(TAURI_COMMANDS.getChkszApiKeyStatus);
}

export function setChkszApiKey(apiKey: string) {
  return invoke<void>(TAURI_COMMANDS.setChkszApiKey, { apiKey });
}

export function clearChkszApiKey() {
  return invoke<void>(TAURI_COMMANDS.clearChkszApiKey);
}
