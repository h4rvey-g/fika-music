import Foundation
import Tauri
import UIKit
import WebKit

private let loginURL = URL(string: "https://music.163.com/#/login")!
private let sessionKeys: Set<String> = ["MUSIC_U", "__csrf", "NMTID", "MUSIC_A"]
private let browserUA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"

private func allowedNavigation(_ url: URL) -> Bool {
    url.scheme == "https" && url.user == nil && url.password == nil &&
        (url.port == nil || url.port == 443) && ["music.163.com", "st.music.163.com"].contains(url.host ?? "")
}

private struct SessionArgs: Decodable { let sessionId: String }

private final class LoginController: UIViewController, WKNavigationDelegate, WKUIDelegate {
    let browser: WKWebView
    var cancelled = false

    init() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        // No script message handlers or Tauri initialization scripts are installed.
        browser = WKWebView(frame: .zero, configuration: configuration)
        super.init(nibName: nil, bundle: nil)
        browser.navigationDelegate = self
        browser.uiDelegate = self
        browser.customUserAgent = browserUA
        modalPresentationStyle = .fullScreen
    }

    required init?(coder: NSCoder) { return nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        let toolbar = UINavigationBar()
        let item = UINavigationItem(title: "music.163.com")
        item.rightBarButtonItem = UIBarButtonItem(barButtonSystemItem: .cancel, target: self, action: #selector(close))
        toolbar.setItems([item], animated: false)
        let children: [UIView] = [toolbar, browser]
        for child in children {
            child.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(child)
        }
        NSLayoutConstraint.activate([
            toolbar.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            toolbar.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            toolbar.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            browser.topAnchor.constraint(equalTo: toolbar.bottomAnchor),
            browser.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            browser.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            browser.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor)
        ])
        browser.load(URLRequest(url: loginURL))
    }

    @objc func close() {
        cancelled = true
        browser.stopLoading()
        dismiss(animated: true)
    }

    func navigationAllowed(_ url: URL, mainFrame: Bool) -> Bool {
        // The official desktop site embeds its login page in a same-origin iframe.
        allowedNavigation(url) || (!mainFrame && url.absoluteString == "about:blank")
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let allowed = action.targetFrame != nil && action.request.url.map {
            navigationAllowed($0, mainFrame: action.targetFrame?.isMainFrame ?? true)
        } == true
        decisionHandler(allowed ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        decisionHandler(response.canShowMIMEType ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }
}

class NeteaseLoginPlugin: Plugin {
    private var sessionId: String?
    private var controller: LoginController?

    @objc public func start(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SessionArgs.self)
        DispatchQueue.main.async {
            guard self.controller == nil, let parent = self.manager.viewController,
                  parent.presentedViewController == nil else {
                invoke.reject("Could not present the official login")
                return
            }
            let login = LoginController()
            self.sessionId = args.sessionId
            self.controller = login
            parent.present(login, animated: true) { invoke.resolve() }
        }
    }

    @objc public func poll(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SessionArgs.self)
        DispatchQueue.main.async {
            guard self.sessionId == args.sessionId, let login = self.controller,
                  !login.cancelled, login.presentingViewController != nil else {
                invoke.resolve(["cancelled": true])
                return
            }
            if let url = login.browser.url, !allowedNavigation(url) {
                login.close()
                invoke.reject("Untrusted login navigation was rejected")
                return
            }
            login.browser.configuration.websiteDataStore.httpCookieStore.getAllCookies { cookies in
                guard !login.cancelled, self.sessionId == args.sessionId else {
                    invoke.resolve(["cancelled": true])
                    return
                }
                let header = cookies.filter {
                    sessionKeys.contains($0.name) && $0.path == "/" &&
                        ["music.163.com", ".music.163.com"].contains($0.domain)
                }.map { "\($0.name)=\($0.value)" }.joined(separator: "; ")
                invoke.resolve(["cancelled": false, "cookieHeader": header])
            }
        }
    }

    @objc public func cancel(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SessionArgs.self)
        DispatchQueue.main.async {
            if self.sessionId == args.sessionId {
                self.controller?.close()
                self.controller = nil
                self.sessionId = nil
            }
            invoke.resolve()
        }
    }
}

@_cdecl("init_plugin_netease_login")
func initPlugin() -> Plugin { NeteaseLoginPlugin() }

