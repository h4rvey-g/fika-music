package com.hvg.neteaselogin

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LoginPolicyTest {
    @Test
    fun allowsOfficialLoginAndSecurityVerification() {
        for (url in listOf(
            "https://music.163.com/#/login",
            "https://music.163.com:443/",
            "https://st.music.163.com/encrypt-pages?qrCode=fixture",
        )) assertTrue(url, LoginPolicy.allowedNavigation(url))
    }

    @Test
    fun rejectsUntrustedNavigationAndSchemeEscapes() {
        for (url in listOf(
            "http://music.163.com/",
            "https://music.163.com.evil.test/",
            "https://evil.test/",
            "https://music.163.com:444/",
            "https://user@music.163.com/",
            "https://music.163.com@evil.test/",
            "tauri://localhost/",
            "file:///data/data/secret",
            "javascript:alert(1)",
            "intent://music.163.com/",
            "not a URL",
        )) assertFalse(url, LoginPolicy.allowedNavigation(url))
    }
}
