import { describe, expect, it } from "vitest";
import { neteaseVerificationNotice } from "./netease-verification";

const challenge = {
  url: "https://st.music.163.com/encrypt-pages?qrCode=fixture-only",
  qrImageDataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
};
const rejection = {
  code: "verification-required",
  message: "NetEase API rejected send verification code (code -462): verification required",
  verification: challenge,
};

describe("neteaseVerificationNotice", () => {
  it("recognizes the structured verification error and its official challenge", () => {
    expect(neteaseVerificationNotice(rejection)).toEqual({
      message: rejection.message,
      challenge,
    });
  });

  it("recognizes JSON-serialized command errors", () => {
    expect(neteaseVerificationNotice(JSON.stringify(rejection))?.challenge).toEqual(challenge);
  });

  it.each([
    "https://example.test/encrypt-pages",
    "https://st.music.163.com.example.test/encrypt-pages",
    "https://st.music.163.com/other-page",
    "https://st.music.163.com:8443/encrypt-pages",
    "https://user:pass@st.music.163.com/encrypt-pages",
    "http://st.music.163.com/encrypt-pages",
    "javascript:alert(1)",
    "not a URL",
  ])("does not offer an unsafe verification link: %s", (url) => {
    expect(neteaseVerificationNotice({
      ...rejection,
      verification: { ...challenge, url },
    })?.challenge).toBeNull();
  });

  it("still recognizes a required verification without challenge parameters", () => {
    expect(neteaseVerificationNotice({ ...rejection, verification: null })?.challenge).toBeNull();
  });

  it("recognizes the legacy verification-required message", () => {
    expect(neteaseVerificationNotice({
      code: "api-failure",
      message: "NetEase API rejected send verification code (code -462): \u8bf7\u5b8c\u6210\u9a8c\u8bc1\u64cd\u4f5c",
    })).not.toBeNull();
  });

  it("does not mistake the operation name for an upstream verification request", () => {
    expect(neteaseVerificationNotice({
      code: "api-failure",
      message: "NetEase API rejected send verification code (code -462): network is crowded",
    })).toBeNull();
  });

  it.each([null, [], "invalid JSON", new Error("Network unavailable"), { code: "bridge-failure", message: "verification" }])(
    "does not turn unrelated errors into security challenges: %s",
    (error) => expect(neteaseVerificationNotice(error)).toBeNull(),
  );
});
