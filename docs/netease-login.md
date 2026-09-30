# NetEase Login Compatibility

Login requests use `src-tauri/src/netease/login.rs`. Music queries still use the
pinned `netease-music` crate. No Node runtime or remote API proxy is introduced.

## Upstream References

- [PR #201](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/pull/201),
  head `b7bf2b527c5c0b2e67515f1573a4aa40f3b556fa`: web QR login and mobile SMS
  login. This PR is unmerged; adopting its behavior is not evidence that a real
  account will pass NetEase risk control.
- The [`login.js`](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/main/module/login.js)
  and [`login_cellphone.js`](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/main/module/login_cellphone.js)
  modules: account-password login sends an MD5 digest rather than the plaintext
  password. In the inspected upstream snapshot, phone login explicitly selects
  weapi, but email login uses the request layer's default eapi mode. Fika
  currently uses weapi for both; this is a compatibility difference, not a
  verified explanation for the front-risk QR endpoint returning code `301`.
- [PR #243](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/pull/243),
  commit `22003c2c322387aba696232abf74a0163aebf723`: server-issued NMTID handling.
- [Third-party login research](netease-login-research.md), inspected on
  2026-09-30: recent client snapshots, official-web login alternatives, and
  limits of the available evidence for password-login risk control.

## Adopted Behavior

- Desktop login defaults to the official NetEase website in a separate,
  incognito native WebView. Credentials and interactive security checks remain
  on `https://music.163.com/#/login`; Fika does not collect the password in its
  own frontend. SMS and QR login remain available.
- The official-login window has no application capability grants, and the host
  command handler rejects application commands from its window label. Navigation
  is restricted to HTTPS on `music.163.com` and `st.music.163.com`, with new
  windows and downloads denied.
- The host reads only root-path `MUSIC_U`, `__csrf`, `NMTID`, and `MUSIC_A`
  cookies from the music origin. It confirms a non-anonymous account before
  using the existing secure persistence path. The frontend receives only a
  temporary session id and the confirmed account, never raw cookies.
- Only one official-login attempt can be active. Closing the native window,
  cancelling, switching methods, or unmounting the login panel stops the attempt;
  the host expires it after ten minutes. Cancellation during account verification
  prevents the later response from committing a session.
- Official-web session import is desktop-only because Tauri's native cookie
  retrieval is unsupported on Android. Mobile platforms keep SMS and QR login.
- Web QR key and polling requests use weapi, `type: 1`, and browser request
  headers. The scan URL is `/st/platform/scanlogin`. One `chainId` and cookie
  context are retained for the entire session.
- Account-password login accepts an email account or an 11-digit mainland China
  phone number. It uses the `/api/w/login` and `/api/w/login/cellphone` payloads
  from api-enhanced, hashes the password with MD5 before weapi encryption, and
  never stores or logs the plaintext password. Upstream's default email
  transport differs as described above. This direct API remains available in the
  bridge, but the application password form has been replaced by official-web
  login after the real-account risk-control failures below.
- If account-password login triggers front-risk verification, the generated
  verification challenge carries a bounded in-memory session. Retrying after
  the external check reuses the same web cookies and login request context;
  only the MD5 request payload is retained while the challenge is pending.
- Before a web QR key or account-password login, the same login client registers
  an anonymous session with a 52-character uppercase hexadecimal `deviceId` and
  retains the server-issued `MUSIC_A` cookie. A rejected registration or
  missing cookie stops the flow without falling back to an unauthenticated
  request. The guest cookie is not treated as a connected account.
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
- On Android, login requests use a bundled WebPKI root store with rustls certificate
  and hostname verification. This avoids reqwest's platform verifier, which
  requires JVM/Kotlin initialization not present in the app. User-installed
  Android CAs are not trusted by this login client; other platforms keep their
  existing TLS configuration.

## Security Verification Boundary

Code `8821` is a verification-required error, not a successful login. The QR
stays visible and automatic polling pauses. `poll_netease_qr_login` accepts
optional `secureCaptcha` and `ydDeviceToken` from a genuinely completed check.
These values are not logged or written to persistent credentials.

The PR's examples do not implement a complete interactive Yidun challenge, and
Fika does not invent one. Acquiring that validation result through an official
interactive challenge remains separate work. The existing front-risk QR dialog
still requires an actual `data.qrCode`; a null response never becomes a link.

The direct real-account password test remains blocked: the login returns `-462`, and
the verification-QR request returns HTTP `200` with API code `301`, even after
anonymous `MUSIC_A` registration. This establishes an upstream rejection, not
its root cause. Offline challenge and cookie-reuse tests do not establish that
real-account password login works.

Offline tests cover request headers, encrypted responses, cookie reuse,
guest registration before QR key creation, verification pause/resume, expired
sessions, and successful account persistence. A one-shot anonymous desktop
probe moved the initial QR poll from `-462` to `801` (waiting for scan). A
separate macOS smoke build subsequently connected an account after a real scan
and account confirmation. On 2026-09-30, the user confirmed that the new
official-web login and session import succeeded on macOS. This is a
user-reported real-account result, not an automated test or confirmation of
direct password login. Background GUI inspection showed blank WebViews,
including the development main window; its cause was not established and no
navigation or security guard was relaxed to work around it. Android still
requires a new build and device retest.

Official-web import tests additionally cover account confirmation, guest and
expired-session rejection, cancellation before persistence, trusted cookie and
navigation filtering, late frontend responses, and import failures.
