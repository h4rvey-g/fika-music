package com.hvg.neteaselogin

import java.net.URI
import java.net.URISyntaxException

internal object LoginPolicy {
    fun allowedNavigation(value: String): Boolean = try {
        val uri = URI(value)
        uri.scheme == "https" && uri.rawUserInfo == null &&
            (uri.port == -1 || uri.port == 443) && uri.host in setOf("music.163.com", "st.music.163.com")
    } catch (_: URISyntaxException) {
        false
    }
}
