# NetEase Login Compatibility

Login requests use `src-tauri/src/netease/login.rs`. Music queries still use the
pinned `netease-music` crate. No Node runtime or remote API proxy is introduced.

## Upstream References

- [PR #201](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/pull/201),
  head `b7bf2b527c5c0b2e67515f1573a4aa40f3b556fa`: web QR login and mobile SMS
  login. This PR is unmerged; adopting its behavior is not evidence that a real
  account will pass NetEase risk control.
- [PR #243](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/pull/243),
  commit `22003c2c322387aba696232abf74a0163aebf723`: server-issued NMTID handling.

## Adopted Behavior

- Web QR key and polling requests use weapi, `type: 1`, and browser request
  headers. The scan URL is `/st/platform/scanlogin`. One `chainId` and cookie
  context are retained for the entire session.
- SMS sending and login use mobile eapi against `interface3.music.163.com`,
  with the PR's iOS/RN login fields, device headers, and empty encrypted header.
- Mobile eapi starts without NMTID, retains the server-issued value, and carries
  it in subsequent request cookies. After three completed requests without a
  server-issued value, the upstream fallback is generated once and retained.
  No additional probing requests are sent. Web cookie initialization follows
  PR #201, and also retains any replacement NMTID returned by the server.
- Both HTTP `Set-Cookie` and the JSON `cookie` field are retained. Account
  confirmation uses the same context before the existing secure persistence
  path is called.
- Mobile responses support AES-ECB/PKCS7 decryption and gzip on either side of
  encryption, with bounded reads and decompression.
- Login requests are not retried or redirected automatically. Pending sessions
  retain the existing expiry and count limits.

## Security Verification Boundary

Code `8821` is a verification-required error, not a successful login. The QR
stays visible and automatic polling pauses. `poll_netease_qr_login` accepts
optional `secureCaptcha` and `ydDeviceToken` from a genuinely completed check.
These values are not logged or written to persistent credentials.

The PR's examples do not implement a complete interactive Yidun challenge, and
Fika does not invent one. Acquiring that validation result through an official
interactive challenge remains separate work. The existing front-risk QR dialog
still requires an actual `data.qrCode`; a null response never becomes a link.

Offline tests cover request headers, encrypted responses, cookie reuse,
verification pause/resume, expired sessions, and successful account persistence.
Real Android account login still requires a device retest with a new build.
