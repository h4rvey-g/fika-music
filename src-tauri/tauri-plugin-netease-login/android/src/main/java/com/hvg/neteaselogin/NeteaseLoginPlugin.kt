package com.hvg.neteaselogin

import android.app.Activity
import android.app.Dialog
import android.graphics.Color
import android.net.http.SslError
import android.os.Message
import android.view.ViewGroup
import android.view.Window
import android.webkit.CookieManager
import android.webkit.SslErrorHandler
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

private const val LOGIN_URL = "https://music.163.com/#/login"
private const val COOKIE_URL = "https://music.163.com/"
private const val USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
private val SESSION_COOKIES = setOf("MUSIC_U", "__csrf", "NMTID", "MUSIC_A")

@InvokeArg
class SessionArgs { lateinit var sessionId: String }

@TauriPlugin
class NeteaseLoginPlugin(private val activity: Activity) : Plugin(activity) {
    private var sessionId: String? = null
    private var browser: WebView? = null
    private var dialog: Dialog? = null
    private var cancelled = true

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(SessionArgs::class.java)
        activity.runOnUiThread {
            if (dialog != null) {
                invoke.reject("An official login is already open")
                return@runOnUiThread
            }
            sessionId = args.sessionId
            cancelled = false
            clearSessionCookies {
                if (cancelled || sessionId != args.sessionId) {
                    invoke.reject("Official login was cancelled")
                } else if (sessionHeader().split(';').any { it.trim().startsWith("MUSIC_U=") }) {
                    cancelled = true
                    invoke.reject("Could not clear the previous official login")
                } else {
                    try {
                        showBrowser()
                        invoke.resolve()
                    } catch (_: Exception) {
                        closeBrowser()
                        clearSessionCookies { invoke.reject("Could not open the official website") }
                    }
                }
            }
        }
    }

    @Command
    fun poll(invoke: Invoke) {
        val args = invoke.parseArgs(SessionArgs::class.java)
        activity.runOnUiThread {
            val closed = cancelled || sessionId != args.sessionId || dialog?.isShowing != true
            val result = JSObject().put("cancelled", closed)
            if (!closed) {
                val url = browser?.url
                if (url != null && !LoginPolicy.allowedNavigation(url)) {
                    closeBrowser()
                    invoke.reject("Untrusted login navigation was rejected")
                    return@runOnUiThread
                }
                result.put("cookieHeader", sessionHeader())
            }
            invoke.resolve(result)
        }
    }

    @Command
    fun cancel(invoke: Invoke) {
        val args = invoke.parseArgs(SessionArgs::class.java)
        activity.runOnUiThread {
            if (sessionId == args.sessionId) {
                closeBrowser()
                clearSessionCookies { invoke.resolve() }
            } else invoke.resolve()
        }
    }

    private fun showBrowser() {
        val web = WebView(activity)
        browser = web
        web.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            userAgentString = USER_AGENT
            allowFileAccess = false
            allowContentAccess = false
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            useWideViewPort = true
            loadWithOverviewMode = true
            builtInZoomControls = true
            displayZoomControls = false
            setSupportMultipleWindows(true)
        }
        CookieManager.getInstance().setAcceptThirdPartyCookies(web, false)
        // A plain WebView has no Tauri IPC or application JavaScript interface.
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                !LoginPolicy.allowedNavigation(request.url.toString())

            @Deprecated("Required for older WebView navigation callbacks")
            override fun shouldOverrideUrlLoading(view: WebView, url: String): Boolean =
                !LoginPolicy.allowedNavigation(url)

            override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
                handler.cancel()
            }
        }
        web.webChromeClient = object : WebChromeClient() {
            override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, message: Message): Boolean = false
        }
        web.setDownloadListener { _, _, _, _, _ -> }
        val layout = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.WHITE)
        }
        val toolbar = LinearLayout(activity).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        toolbar.addView(TextView(activity).apply {
            text = "music.163.com"
            setTextColor(Color.BLACK)
            setPadding(dp(16), dp(12), dp(16), dp(12))
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        toolbar.addView(ImageButton(activity).apply {
            setImageResource(android.R.drawable.ic_menu_close_clear_cancel)
            contentDescription = activity.getString(android.R.string.cancel)
            setBackgroundColor(Color.TRANSPARENT)
            setOnClickListener { dialog?.dismiss() }
        }, LinearLayout.LayoutParams(dp(48), dp(48)))
        layout.addView(toolbar)
        layout.addView(web, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        val loginDialog = Dialog(activity)
        loginDialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        loginDialog.setContentView(layout)
        loginDialog.setOnDismissListener {
            closeBrowser()
            clearSessionCookies {}
        }
        dialog = loginDialog
        loginDialog.show()
        loginDialog.window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        web.loadUrl(LOGIN_URL)
    }

    private fun closeBrowser() {
        cancelled = true
        val previous = dialog
        dialog = null
        previous?.setOnDismissListener(null)
        previous?.dismiss()
        browser?.apply {
            stopLoading()
            (parent as? ViewGroup)?.removeView(this)
            clearHistory()
            destroy()
        }
        browser = null
    }

    private fun sessionHeader(): String = CookieManager.getInstance().getCookie(COOKIE_URL)
        .orEmpty().split(';').filter { it.trim().substringBefore('=') in SESSION_COOKIES }.joinToString(";")

    private fun dp(value: Int): Int = (value * activity.resources.displayMetrics.density).toInt()

    private fun clearSessionCookies(done: () -> Unit) {
        // Android shares CookieManager with the main WebView. Delete only NetEase session keys.
        val manager = CookieManager.getInstance()
        val domains = listOf("", "; Domain=music.163.com", "; Domain=.music.163.com", "; Domain=.163.com")
        var remaining = SESSION_COOKIES.size * domains.size
        for (name in SESSION_COOKIES) for (domain in domains) {
            manager.setCookie(COOKIE_URL, "$name=; Path=/; Max-Age=0; Secure$domain") {
                remaining -= 1
                if (remaining == 0) {
                    manager.flush()
                    done()
                }
            }
        }
    }

    override fun onDestroy(activity: AppCompatActivity) {
        closeBrowser()
        clearSessionCookies {}
    }

}

