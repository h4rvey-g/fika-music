import type { NeteaseVerificationChallenge } from "../generated/bindings";

export type NeteaseVerificationNotice = {
  message: string;
  challenge: NeteaseVerificationChallenge | null;
};

export function neteaseVerificationNotice(error: unknown): NeteaseVerificationNotice | null {
  let value = error;
  if (typeof value === "string") {
    try { value = JSON.parse(value) as unknown; } catch { return null; }
  }
  if (!isRecord(value) || typeof value.message !== "string") return null;
  const legacyMessage = /^NetEase API rejected [^(]*\(code (?:-462|460)\): (.*)$/s.exec(value.message)?.[1] ?? "";
  const required = value.code === "verification-required" || (
    value.code === "api-failure"
    && /\u9a8c\u8bc1|verification/i.test(legacyMessage)
  );
  if (!required) return null;

  const verification = value.verification;
  let challenge: NeteaseVerificationChallenge | null = null;
  if (isRecord(verification)
    && typeof verification.url === "string"
    && typeof verification.qrImageDataUrl === "string"
    && verification.qrImageDataUrl.startsWith("data:image/svg+xml;base64,")
  ) {
    try {
      const url = new URL(verification.url);
      if (url.origin === "https://st.music.163.com"
        && url.pathname === "/encrypt-pages"
        && !url.username && !url.password
      ) {
        challenge = { url: verification.url, qrImageDataUrl: verification.qrImageDataUrl };
      }
    } catch { /* Invalid verification URLs remain unavailable. */ }
  }
  return { message: value.message, challenge };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
