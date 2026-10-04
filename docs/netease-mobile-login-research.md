# 网易云音乐第三方手机客户端登录调研

调研日期：2026-10-04。目的：补充此前偏桌面端的[登录调研](netease-login-research.md)，寻找近期维护的 Android／iOS 客户端，核查实际登录入口、请求和 Cookie 管理，并对照 Fika 的手机实现。

## 结论

最适合 Fika 借鉴的是 **FMP 的 Android 官方网页登录**：在手机内打开官方登录页，通过原生 CookieManager 取得会话，再请求账号信息验证。它在 2026-09-24 发布了 Android APK，已发布版本与当前主分支的登录页面一致。Fika 已采用相同的主流程。本轮进一步应用了 **手机浏览器 UA／显示模式** 的借鉴点，具体变更见下方应用记录；是否改善真实账号登录仍需设备验证。

这次核查没有找到能证明“近期第三方手机端已完整解决 `-462`／`8821` 风控挑战”的实现，也没有在下面五个项目已检查的登录代码中找到完整的同机官方 App 授权跳转。二维码页面存在，并不代表一部手机就能方便地完成扫码。

上述结论来自源码、GitHub 发布记录和提交记录；本轮没有使用真实账号登录这些应用。发布了 APK、代码调用了接口，都不能单独证明当前账号一定可以通过网易云风控。

## 项目与更新记录

“源码日期”是所检查分支顶端提交的日期；“发布日期”是 GitHub Release 的 `published_at`。两者均不代表登录功能最后修复的日期。

| 项目 | 手机平台 | 源码日期／固定快照 | 最近核实的发布 | 登录实现与参考价值 |
| --- | --- | --- | --- | --- |
| [FMP](https://github.com/1morr/FMP) | Android；另有 Windows | 2026-10-03，[`c0a87b4`](https://github.com/1morr/FMP/commit/c0a87b402a801ead19a7b96ea54302334a3f99c0) | [v1.11.0](https://github.com/1morr/FMP/releases/tag/v1.11.0)，2026-09-24，有 APK | Android 默认官方 WebView 登录，另有二维码；直接请求网易云，安全存储 Cookie。最值得参考。 |
| [ML-Netease Android](https://github.com/midairlogn/ML-Netease_Android) | Android | 2026-10-03，[`c4088b5`](https://github.com/midairlogn/ML-Netease_Android/commit/c4088b579723f1a30f5c4171b09028ba9bbf1052) | [v2.7.5](https://github.com/midairlogn/ML-Netease_Android/releases/tag/v2.7.5)，2026-10-03，有 APK | 设置页手动填 `MUSIC_U`；没有交互式登录实现可借鉴。 |
| [波尼音乐／PonyMusic](https://github.com/wangchenyan/ponymusic) | Android 原生 | 2026-09-16，[`f3c502c`](https://github.com/wangchenyan/ponymusic/commit/f3c502c9e9d448174c644d77dc0492c7b59d555b) | [2.4.0-beta01](https://github.com/wangchenyan/ponymusic/releases/tag/2.4.0-beta01)，2025-10-30，有 APK | 手机验证码和二维码；依赖用户自建网易云 API 服务。验证码登录文件最近提交为 2024-01-04。 |
| [CyreneMusic](https://github.com/moraxs/CyreneMusic) | Flutter；README 列出 Android／iOS | 2026-08-19，[`8a5633f`](https://github.com/moraxs/CyreneMusic/commit/8a5633f5392f0bbfd2a3dfe3df05bcf8a9185e0a) | [1.2.5](https://github.com/moraxs/CyreneMusic/releases/tag/1.2.5)，2026-01-29，有 APK；发布说明指引 iOS 从 Actions 下载 | 二维码和后端账号绑定；客户端依赖第三方／自定义后端，不是本地原生会话导入。 |
| [不倦／Bujuan](https://github.com/2697a/bujuan) | Flutter 手机／桌面；公开稳定产物有 Android | 2025-12-19，默认分支 `feature/new-ui`，[`44698e4`](https://github.com/2697a/bujuan/commit/44698e454d84d3f9dc68ce06c578781d05f3bc1a) | [2.0.5](https://github.com/2697a/bujuan/releases/tag/2.0.5)，2023-12-14，有 APK | 当前分支是短信验证码界面；接口实现位于未取得的子模块，不能据此确认传输层或当前可用性。 |

发布记录可通过 GitHub 的第一方 API 复核，例如 [FMP releases](https://api.github.com/repos/1morr/FMP/releases?per_page=2)、[ML-Netease releases](https://api.github.com/repos/midairlogn/ML-Netease_Android/releases?per_page=2)、[PonyMusic releases](https://api.github.com/repos/wangchenyan/ponymusic/releases?per_page=2)、[CyreneMusic releases](https://api.github.com/repos/moraxs/CyreneMusic/releases?per_page=2) 和 [Bujuan releases](https://api.github.com/repos/2697a/bujuan/releases?per_page=2)。

## FMP：已发布的手机 WebView 登录

主要源码：

- [Android 登录页面，v1.11.0 固定版本](https://github.com/1morr/FMP/blob/2d214e1eb8c3e6f764a86e55bf27071295e02c9d/lib/ui/pages/settings/netease_login_page.dart)。该文件与所检查主分支快照的 `git diff` 为空。
- [账号服务，所检查主分支快照](https://github.com/1morr/FMP/blob/c0a87b402a801ead19a7b96ea54302334a3f99c0/lib/services/account/netease_account_service.dart)。账号服务文件与 v1.11.0 的 `git diff --stat` 也为空。
- [凭据结构](https://github.com/1morr/FMP/blob/c0a87b402a801ead19a7b96ea54302334a3f99c0/lib/services/account/netease_credentials.dart)。

实际流程：

1. Android 页面显示两个标签，默认第一个是网页登录，第二个是二维码。桌面端只有二维码。
2. `flutter_inappwebview` 打开 `https://music.163.com/#/login`，启用 JavaScript 和 DOM storage。
3. WebView 使用 **Android 14／Pixel 8／Chrome 122 的 Mobile UA**，并设置 `UserPreferredContentMode.MOBILE`。
4. 在 `onLoadStop` 时，通过原生 `CookieManager.getCookies` 查询固定 `https://music.163.com` URL。取得非空 `MUSIC_U` 后，同时取 `__csrf`。
5. 调用 `loginWithCookiesAndValidate`。该函数先保存临时新状态，再请求账号信息；失败则恢复原有凭据和账号快照。
6. 账号检查先 GET `/api/nuser/account/get`；网络请求抛错时再 POST `/api/w/nuser/account/get`。响应必须是 `code=200` 且有 `profile`，否则不能算登录成功。
7. 凭据保存在 secure storage，成功后清理 WebView 并关闭登录页面。

这是 **官方网页完成认证 → 原生读取 Cookie → 验证账号**，没有自行提交手机号密码，也没有实现短信风控破解。请求层直接访问网易云域名，不需要独立 Node API 服务。

它的二维码方式则用 weapi 获取 unikey，组成 `https://music.163.com/login?codekey=...`，每三秒检查状态；取得 `803` 后从 `Set-Cookie` 或响应 body 取凭据，再走同一个账号验证入口。轮询有次数和连续错误上限。已检查的二维码页只显示二维码，没有同机 `orpheus://` 授权按钮或保存二维码到相册的入口。

需要区分网页 UA 与 API UA：FMP 网页用 Mobile UA，但账号 API 请求仍使用桌面网易云 UA。不能把这项源码观察扩展成“所有手机 API 都应该统一改用手机 UA”。

FMP 使用 [MIT 许可](https://github.com/1morr/FMP/blob/c0a87b402a801ead19a7b96ea54302334a3f99c0/LICENSE)。本轮只研究行为，没有复制实现。

## 波尼音乐：原生界面，认证交给自建 API 服务

主要源码：

- [AccountApi.kt](https://github.com/wangchenyan/ponymusic/blob/f3c502c9e9d448174c644d77dc0492c7b59d555b/app/src/main/java/me/wcy/music/account/AccountApi.kt)。
- [PhoneLoginViewModel.kt](https://github.com/wangchenyan/ponymusic/blob/f3c502c9e9d448174c644d77dc0492c7b59d555b/app/src/main/java/me/wcy/music/account/login/phone/PhoneLoginViewModel.kt)。
- [QrcodeLoginViewModel.kt](https://github.com/wangchenyan/ponymusic/blob/f3c502c9e9d448174c644d77dc0492c7b59d555b/app/src/main/java/me/wcy/music/account/login/qrcode/QrcodeLoginViewModel.kt)。
- [UserServiceImpl.kt](https://github.com/wangchenyan/ponymusic/blob/f3c502c9e9d448174c644d77dc0492c7b59d555b/app/src/main/java/me/wcy/music/account/service/UserServiceImpl.kt)。

短信发送是 `GET captcha/sent?phone=...`，验证码登录是 `GET login/cellphone?phone=...&captcha=...`。这些路径属于用户配置的 API 服务，不能直接当成网易云官方接口路径。发送成功后有 30 秒倒计时；登录返回 `code=200` 和 Cookie 后，再请求 `login/status` 验证账号。

二维码由 `/login/qr/key` 和 `/login/qr/create` 生成，约三秒一次查询 `/login/qr/check`，`803` 后验证账号。界面提示用网易云 App 扫码，未发现已检查页面中的同机授权或相册保存入口。

`PhoneLoginViewModel` 的异常路径遇到 `-462` 时，仅把提示替换为“登录失败，请更新服务端版本或稍后重试”。没有调用 `frontrisk/verify/getqrcode`，也没有完整的易盾挑战处理。

[README](https://github.com/wangchenyan/ponymusic/blob/f3c502c9e9d448174c644d77dc0492c7b59d555b/README.md) 明确要求自行部署 API 服务。这个项目可参考验证码页面和登录后验证，但它没有提供比 Fika 现有 Rust 手机 eapi 更完整的认证传输层。

最近的账号验证码文件记录可见 [GitHub path commits](https://api.github.com/repos/wangchenyan/ponymusic/commits?per_page=3&path=app/src/main/java/me/wcy/music/account/login/phone/PhoneLoginViewModel.kt)：2024-01-04。2026 年的应用更新主要不能用来证明验证码兼容性刚被修复。

## CyreneMusic：扫码后绑定到后端账号

主要源码：

- [netease_login_service.dart](https://github.com/moraxs/CyreneMusic/blob/8a5633f5392f0bbfd2a3dfe3df05bcf8a9185e0a/lib/services/netease_login_service.dart)。
- [netease_qr_dialog.dart](https://github.com/moraxs/CyreneMusic/blob/8a5633f5392f0bbfd2a3dfe3df05bcf8a9185e0a/lib/pages/settings_page/netease_qr_dialog.dart)。
- [url_service.dart](https://github.com/moraxs/CyreneMusic/blob/8a5633f5392f0bbfd2a3dfe3df05bcf8a9185e0a/lib/services/url_service.dart)。

客户端请求可配置后端的 `/login/qr/key`，自行组成 `https://music.163.com/login?codekey=...` 并绘制二维码；约每两秒请求 `/login/qr/check?key=...&userId=...`。`803` 时关闭对话框，提示“网易云账号绑定成功”。后续用该项目自己的 Bearer token 查询 `/accounts/bindings` 和用户网易云歌单。

在所检查客户端代码中，`NeteaseQrCheckResult` 没有 Cookie 字段，也没有把网易云 Cookie 导入本地原生会话的流程。后端源码不在这份仓库快照中，所以只能确认客户端的绑定协议，不能确认后端如何保存凭据或处理风控。

仓库里的[扫码 API 说明](https://github.com/moraxs/CyreneMusic/blob/8a5633f5392f0bbfd2a3dfe3df05bcf8a9185e0a/docs/netease-qr-login-guide.md) 带有直接请求示例，但当前 App 实际执行的服务是上述后端调用。说明里的错误码归因和示例代码不等于已运行成功的客户端实现。

README 与发布说明声明支持 iOS，但本轮未验证 IPA 构建或 iOS 真机登录。它没有提供已核实的 iOS 原生网页登录 Cookie 采集参考。

## ML-Netease Android：最新 APK，手动 Cookie

[SettingsFragment.java](https://github.com/midairlogn/ML-Netease_Android/blob/c4088b579723f1a30f5c4171b09028ba9bbf1052/app/src/main/java/com/midairlogn/mlnetease/settings/SettingsFragment.java) 在设置页提供 `MUSIC_U` 输入框，[SettingsManager.java](https://github.com/midairlogn/ML-Netease_Android/blob/c4088b579723f1a30f5c4171b09028ba9bbf1052/app/src/main/java/com/midairlogn/mlnetease/settings/SettingsManager.java) 将值保存到 SharedPreferences。

[NeteaseApi.java](https://github.com/midairlogn/ML-Netease_Android/blob/c4088b579723f1a30f5c4171b09028ba9bbf1052/app/src/main/java/com/midairlogn/mlnetease/network/NeteaseApi.java) 在请求中附加 `MUSIC_U`；OkHttp CookieJar 不保存服务端 Cookie。v2.7.5 发布说明也明确写的是设置 `Music_U` Cookie。

这个项目说明最近仍有客户端采用手动导入凭据，但不能用于借鉴自动网页登录或短信登录，也不建议用普通偏好设置替代 Fika 的安全凭据存储。

## 不倦：当前重写分支的短信界面

[login_page.dart](https://github.com/2697a/bujuan/blob/44698e454d84d3f9dc68ce06c578781d05f3bc1a/lib/pages/login/login_page.dart) 调用 `BujuanMusicManager().sendSmsCode`，输入验证码后调用 `loginCellPhone(phone, captcha)`；只有 `code=200` 且获取到用户 profile 时才进入主页。

页面还显示“QR code login”，但这个位置只是图标和文字，没有点击回调或二维码生成流程，不能据此把当前分支算作已实现扫码登录。重发文字也没有实际发送回调。

传输层依赖 `plugin/bujuan_music_api`。Git tree 记录的是指向 `dff61e6b5989c2cda3b75bffa5057bdd3f65e8ff` 的 gitlink，当前快照没有 `.gitmodules`，普通 shallow clone 没有取得该模块源码。因此未确认它使用 weapi／eapi、Cookie 持久化或风控处理。

当前默认分支的源码不能代表 2023 年发布的 2.0.5 APK。它适合当短信交互参考，不适合作为“近期已修复登录”的证据。

## 对 Fika 的具体建议

Fika 已具备 **独立原生 WebView → 原生读取 Cookie → Rust 验证非匿名账号 → 安全保存** 的主流程，见 [兼容性记录](netease-login.md)、[Android 插件](../src-tauri/tauri-plugin-netease-login/android/src/main/java/com/hvg/neteaselogin/NeteaseLoginPlugin.kt)、[iOS 插件](../src-tauri/tauri-plugin-netease-login/ios/Sources/NeteaseLoginPlugin.swift) 和 [Rust 会话导入](../src-tauri/src/netease.rs)。这次研究支持继续完善这个路径。

| 对比项 | FMP 已确认实现 | Fika 调研时实现 | 建议 |
| --- | --- | --- | --- |
| 官方登录页 | `https://music.163.com/#/login` | 相同 | 保留现有入口。 |
| 手机网页 UA | Android Mobile UA；MOBILE 显示模式 | Android 和 iOS 都是 Windows Chrome 桌面 UA | 优先实机对比当前桌面 UA 与平台原生手机 UA。先确认页面入口、布局、验证过程和 Cookie 导入结果，再决定调整。 |
| Cookie 采集时机 | 页面 `onLoadStop` | 独立轮询，不依赖页面完整刷新 | 保留轮询；单页登录的 Cookie 更新可能不伴随页面加载事件。 |
| 采集字段 | `MUSIC_U`、`__csrf` | 另有 `NMTID`、`MUSIC_A` | 复用现有会话字段，不因参考项目字段少就删掉现有上下文。 |
| 登录后验证 | 保存新状态，再验证；失败回滚 | 先验证非匿名账号并检查取消状态，再提交凭据 | 保留 Fika 当前的验证顺序。 |
| Cookie／缓存清理 | 删除 `.163.com` Cookie，并清全部 WebView cache／storage | Android 只清指定网易云会话 Cookie；iOS 独立临时存储 | 不照搬全局清缓存行为，以免影响主应用 WebView。 |
| 官方导航 | Cookie 检查使用宽泛的 `host.endsWith('163.com')` 判断 | 明确官方 HTTPS 主机白名单，没有应用 IPC | 保留现有导航和 IPC 边界。 |

建议的最小验证步骤：

1. Android 真机对比桌面 UA 和手机 UA：官方登录页是否出现可用的手机号／验证码入口，页面是否完整显示，交互验证是否可完成。
2. 用户完成官方认证后，检查原生端是否取得非空 `MUSIC_U`，以及 Rust 账号确认是否成功。诊断只记录字段存在与否、域、路径和响应码，不记录 Cookie 值、验证码或密码。
3. iOS 单独复测。当前 Cookie 筛选只接受 `music.163.com`／`.music.163.com` 的 root-path Cookie；若真实登录返回其他适用父域 Cookie，先确认原生 CookieStore 元数据，再判断是否需要补充筛选。这里只是待核查点，没有证据表明当前故障一定由域过滤造成。
4. 网络错误、风控、用户取消和确认成功分别验证。完成网页登录的 Cookie 导入不等于 Fika 直接短信／密码 API 的风控问题已解决。

这次没有依据重新加入 Fika 已移除的应用内二维码登录，也没有依据引入远程 API 服务。手机上展示二维码本身不能替代一个能在同机完成的登录路径。

## 2026-10-04 应用记录

- Android 登录窗口改用 `WebSettings.getDefaultUserAgent(activity)`，使浏览器身份跟随设备和已安装 WebView 的版本。继续支持页面 viewport，关闭桌面页面 overview 缩放。
- iOS 去除 Windows Chrome UA 覆盖，使用 WKWebView 默认浏览器身份，并设置 `defaultWebpagePreferences.preferredContentMode = .mobile`，包括 iPad。
- Cookie 轮询、账号确认和安全存储复用已有实现。手机端导航白名单额外接受明确的官方移动登录主机 `y.music.163.com`；网页登录 UA 与 Rust API 请求 UA 分开管理。
- 参考平台 API：[Android WebSettings](https://developer.android.com/reference/android/webkit/WebSettings)、[Apple preferredContentMode](https://developer.apple.com/documentation/webkit/wkwebpagepreferences/preferredcontentmode)。没有照抄参考项目中的固定 Android 型号或 Chrome 版本。
- Edge 只读页面检查发现：Android WebView 和 iPhone 手机浏览器身份都会从 `https://music.163.com/#/login` 跳转到 `https://y.music.163.com/m/login`，并在 390 像素视口显示“手机号登录”。旧白名单拦截这一跳转；新增 Android 回归测试先失败、修正后通过，伪造域名、HTTP、非标准端口和带账号信息的 URL 仍被拒绝。
- Android 原生插件编译及三项导航策略测试、17 项前端登录测试通过；iOS Swift 语法检查及实际导航函数的八项 Foundation 检查通过。没有连接的 Android 真机，当前环境缺少 iOS SDK，尚未完成原生设备上的 Cookie 导入与真实账号登录验证。

## 排除与证据限制

- [JetMelo](https://github.com/rcmiku/JetMelo) 是真正的 Android 客户端，但 GitHub API 显示已归档，源码顶端提交为 2025-03-30，不列为近期维护的优先参考。
- [fluttercandies/NeteaseCloudMusic](https://github.com/fluttercandies/NeteaseCloudMusic) 已归档，顶端提交为 2020-05-22。2026 年发布的教程文章不是项目更新。
- [Listen1 Mobile](https://github.com/listen1/listen1_mobile) 顶端提交为 2022-11-04，最新核实 Release 为 2021-05-24；不是最近更新的登录实现参考。
- [Verse](https://github.com/Brianwind/verse) 在 2026-10-03 发布 v0.4.0，但 README 明确目前只打包 Windows，Release 也只有 Windows 产物。本轮虽检索了其代码，未将它算作已验证的手机端项目。
- [MusicFree](https://github.com/maotoumao/MusicFree) 的主项目明确是插件化播放器，本身不集成平台音源。主应用近期更新不能直接证明某个网易云插件的登录方式或兼容性。
- 本轮二维码、风控和 App 跳转的否定结论只覆盖已检查的五个项目登录界面与服务代码；不扩展到所有客户端、所有历史分支或未取得的后端／子模块。
- iOS 的近期公开产物和本地原生登录源码证据较少。本轮最强参考 FMP 仅支持 Android／Windows，不能把它的 Android 行为当成 iOS 已验证结果。
