# Third-Party NetEase Login Research

Inspected on 2026-09-30. Scope: recently updated third-party clients and their
login behavior relevant to Fika's password-login `-462` and verification-QR
API code `301` failures.

## Evidence Boundary

Octen web searches identified projects and first-party documentation. Source
claims below were checked against cloned repository snapshots. Dates are the
tip commit dates of the inspected branches, not release dates or dates when
login was last fixed. No real account was tested in these third-party clients.
The presence of an endpoint adapter is not evidence of successful login under
current NetEase risk control.

| Project | Inspected Tip Date | Revision | Relevant Behavior |
| --- | --- | --- | --- |
| VutronMusic | 2026-09-26 | [`d5ed97b`](https://github.com/stark81/VutronMusic/commit/d5ed97b267f0a1ee6115c648f066969f2b4f3449) | QR and imported Cookie; current NetEase plugin rejects other login methods. |
| SPlayer-Next | 2026-09-24 | [`ceb9d72`](https://github.com/SPlayer-Dev/SPlayer-Next/commit/ceb9d72b34fe4266674c814f2b3dcd46352a67e5) | QR, official-web session collection, and manual Cookie entry. Password adapters remain in the backend. |
| AlgerMusicPlayer | 2026-09-19 | [`b277ef1`](https://github.com/algerkong/AlgerMusicPlayer/commit/b277ef17a8d6f05152d42528e6930205b95d0fab) | Visible tabs are QR, Cookie, and UID; phone-password code remains but its tab is absent. |
| netease-downloader | 2026-08-08 | [`6dcfeb2`](https://github.com/toki-plus/netease-downloader/commit/6dcfeb295ba06a40f1298ca2d806767796b433e1) | Direct eapi QR login, saving MUSIC_U after confirmation. |
| NetMusic-BetterLogin | 2026-05-14 | [`4c8ce84`](https://github.com/ming-sc/NetMusic-BetterLogin/commit/4c8ce844c20a7b6df6326a7fa5c5bd864887572d) | Email-password, SMS, QR, and Cookie; README acknowledges risk-control failures. |

## Findings

### SPlayer-Next: Delegate Interactive Login to the Official Site

The [current login dialog](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/src/components/modals/LoginDialog.vue)
shows QR login, automatic Cookie collection, and manual Cookie entry, rather
than its own account-password form.

Its [login-window implementation](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/electron/main/window/login.ts)
opens `https://music.163.com/#/login` in a dedicated Electron session partition.
It clears old session storage, lets the user interact with the official page,
and checks cookies once per second. A nonempty `MUSIC_U` is required; it also
collects `__csrf`, `NMTID`, and `MUSIC_A` by URL so both host-only and domain
cookies are considered. This is browser-cookie polling, not repeated password
submission.

The [IPC handler](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/electron/main/ipc/apis.ts)
merges the collected cookies into its session. The dialog subsequently calls
`fetchStatus`; [login-status validation](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/src/apis/login/netease.ts)
rejects anonymous accounts and missing profiles.

Backend [email](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/electron/main/apis/netease/modules/login.ts)
and [phone-password](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/electron/main/apis/netease/modules/login_cellphone.ts)
adapters still exist. They use MD5 password payloads, with email defaulting to
eapi and phone explicitly using weapi. See the [request layer](https://github.com/SPlayer-Dev/SPlayer-Next/blob/ceb9d72b34fe4266674c814f2b3dcd46352a67e5/electron/main/apis/netease/core/request.ts).
Their presence should not be confused with a password-login UI or a complete
front-risk verification implementation.

### VutronMusic: QR and Official-Website Cookie Import

The [NetEase plugin](https://github.com/stark81/VutronMusic/blob/d5ed97b267f0a1ee6115c648f066969f2b4f3449/src/public/plugin/netease.js)
calls a local API service at `http://localhost:41830/netease`. QR confirmation
uses the returned Cookie and checks login status. `doLogin` accepts a Cookie,
validates it through login status, and rejects other login methods. API code
`301` is treated as unauthorized.

The project's [account-login Wiki](https://github.com/stark81/VutronMusic/wiki/%E8%B4%A6%E5%8F%B7%E7%99%BB%E9%99%86),
edited 2025-08-26, instructs users to sign into the official website and import
the Cookie from a logged-in request. It does not document fixing `-462` by
calling the front-risk QR endpoint.

### AlgerMusicPlayer: Retained Password Code Is Not an Exposed Login Method

The [API adapter](https://github.com/algerkong/AlgerMusicPlayer/blob/b277ef17a8d6f05152d42528e6930205b95d0fab/src/renderer/api/login.ts)
still posts `{ phone, password }` to `/login/cellphone`. However, the
[current login view](https://github.com/algerkong/AlgerMusicPlayer/blob/b277ef17a8d6f05152d42528e6930205b95d0fab/src/renderer/views/login/index.vue)
lists only QR, Cookie, and UID tabs. The phone form and function remain but are
not included in that tab list.

UID lookup is public-profile access through `/user/detail`, not proof of an
authenticated session. It cannot replace Fika's `MUSIC_U` credential requirement
for authenticated account operations.

### Other References

- [netease-downloader's direct request implementation](https://github.com/toki-plus/netease-downloader/blob/6dcfeb295ba06a40f1298ca2d806767796b433e1/music_api.py)
  uses eapi QR key creation and polling, and extracts `MUSIC_U` from Set-Cookie
  after code `803`. It does not implement password login.
- [NetMusic-BetterLogin's API implementation](https://github.com/ming-sc/NetMusic-BetterLogin/blob/4c8ce844c20a7b6df6326a7fa5c5bd864887572d/src/main/java/com/github/img/netmusicbetterlogin/api/NeteaseApi.java)
  sends email-password and phone-SMS requests directly as HTTPS GET query
  parameters, using an MD5 password. Its [README](https://github.com/ming-sc/NetMusic-BetterLogin/blob/4c8ce844c20a7b6df6326a7fa5c5bd864887572d/README.md)
  explicitly attributes risk prompts to NetEase risk control and recommends QR
  login. Fika should not adopt credential-bearing URLs, which can leak into
  request logs and diagnostics even when the password is hashed.

## api-enhanced Transport Difference

Reference snapshot: [`a8c781f`](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/commit/a8c781fd64faab17fedfd46e0615a2609307f163),
2026-09-12. The clone is shallow; its tip date does not establish when these
individual endpoint behaviors changed.

- [Email login](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/a8c781fd64faab17fedfd46e0615a2609307f163/module/login.js)
  calls `createOption(query)` without selecting weapi. With the inspected default
  configuration, the [request layer](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/a8c781fd64faab17fedfd46e0615a2609307f163/util/request.js)
  selects eapi, targeting `interfacepc.music.163.com`.
- [Phone login](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/a8c781fd64faab17fedfd46e0615a2609307f163/module/login_cellphone.js)
  explicitly selects weapi.
- [Verification QR creation](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/a8c781fd64faab17fedfd46e0615a2609307f163/module/verify_getQr.js)
  explicitly selects weapi. Its payload matches Fika's
  `verifyConfigId`, `verifyType`, `token`, serialized `params`, and `size`.
- [Anonymous registration](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/a8c781fd64faab17fedfd46e0615a2609307f163/module/register_anonimous.js)
  selects xeapi by default. Fika's current Web preflight uses weapi instead.

These are source-confirmed transport differences, not evidence that changing
one will make the real-account verification QR succeed.

## Implications for Fika

Across these five inspected snapshots, no implementation of
`frontrisk/verify/getqrcode` or explicit `-462` challenge handling was found.
This is a bounded source-search result, not a claim about every third-party
client. SPlayer-Next and VutronMusic both recognize business code `301` as an
authentication failure; in Fika's observed response it is not an HTTP redirect.

Fika's real-account attempt still returns `-462`, followed by HTTP `200` / API
`301` when fetching the verification QR. Anonymous registration did not resolve
that failure. Cookie loss, an endpoint authentication requirement, and request
context differences remain hypotheses; none is established by this research.

The strongest implementation reference is SPlayer-Next's official-web session
collection: let the official site handle its interactive authentication, then
validate the resulting account using the existing Service Bridge and persist
credentials through Fika's secure store. Keep QR login available. This is a
research recommendation, not a real-account verification of those reference
clients. Fika subsequently implemented this flow; on 2026-09-30 the user
confirmed successful macOS official-web login and session import. See
[the compatibility record](netease-login.md) for the implementation boundaries
and verification evidence.

A Fika implementation must remain native Rust/Tauri, without a Node runtime or
remote API proxy. Keep untrusted remote content separate from application
commands and Source Provider scripts, restrict navigation and Cookie collection
to trusted NetEase origins, and retain bounded cancellation and cleanup. Do not
copy Electron's disabled-sandbox setting or its session-storage policy as a
Fika security design. SPlayer-Next is AGPL-3.0; this research references behavior
and does not copy its implementation.
