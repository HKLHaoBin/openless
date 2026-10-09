package com.openless.app

import android.text.InputType
import android.view.inputmethod.EditorInfo
import java.util.Locale

/** Text around the caret, read once when a recording starts. */
internal data class AndroidCursorContext(
    val before: String,
    val after: String,
    val packageName: String?,
)

/**
 * Gates for reading text out of the host editor. Kept apart from
 * [ImeLearningPolicy]: learning decides what stays on this device, while
 * cursor context may be sent to the configured polish LLM provider.
 */
internal object ImePrivacyPolicy {
    // Read a little wider than the 600-char budget; Core trims to the final window.
    const val CURSOR_BEFORE_CHARS = 600
    const val CURSOR_AFTER_CHARS = 200

    private val sensitivePackagePrefixes = setOf(
        "com.x8bit.bitwarden",
        "com.android.keepass",
        "com.kunzisoft.keepass",
        "keepass2android.keepass2android",
        "keepass2android.keepass2android_nonet",
        "com.agilebits.onepassword",
        "com.onepassword.android",
        "com.lastpass.lpandroid",
        "com.dashlane",
        "com.callpod.android_apps.keeper",
        "com.keepersecurity.keeper",
        "com.sinew.enpass",
        "io.enpass.app",
        "proton.android.pass",
        "me.proton.android.pass",
        "com.nordpass",
        "com.nordpass.passwordmanager",
        "com.siber.roboform",
        "com.safeincloud",
        "com.termux",
        "com.server.auditor.ssh.client",
        "org.connectbot",
        "com.sonelli.juicessh",
    )

    /** Single list for cursor context and accessibility vocabulary observation alike. */
    fun isSensitivePackage(packageName: String?): Boolean {
        val name = packageName?.trim()?.lowercase(Locale.ROOT).orEmpty()
        return sensitivePackagePrefixes.any { name == it || name.startsWith("$it.") }
    }

    fun allowsCursorContext(inputType: Int, imeOptions: Int, packageName: String?): Boolean =
        inputType != InputType.TYPE_NULL && !ImeLearningPolicy.isPassword(inputType) &&
            imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING == 0 &&
            !isSensitivePackage(packageName)

    /**
     * One read, no retry: a null, a throw or an empty editor all mean "no
     * context" and dictation carries on. The readers are never invoked while
     * the switch is off or a gate rejects the field.
     */
    fun captureCursorContext(
        enabled: Boolean,
        inputType: Int,
        imeOptions: Int,
        packageName: String?,
        readBefore: (Int) -> CharSequence?,
        readAfter: (Int) -> CharSequence?,
    ): AndroidCursorContext? {
        if (!enabled || !allowsCursorContext(inputType, imeOptions, packageName)) return null
        val before = runCatching { readBefore(CURSOR_BEFORE_CHARS)?.toString() }.getOrNull().orEmpty()
            .let { if (it.firstOrNull()?.isLowSurrogate() == true) it.drop(1) else it }
        val after = runCatching { readAfter(CURSOR_AFTER_CHARS)?.toString() }.getOrNull().orEmpty()
            .let { if (it.lastOrNull()?.isHighSurrogate() == true) it.dropLast(1) else it }
        if (before.isBlank() && after.isBlank()) return null
        return AndroidCursorContext(before, after, packageName)
    }
}
