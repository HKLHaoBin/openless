package com.openless.app

/**
 * JNI bridge from Kotlin overlay / lifecycle code into Rust Coordinator.
 */
object OpenLessNative {
    init {
        try {
            System.loadLibrary("openless_lib")
        } catch (error: UnsatisfiedLinkError) {
            android.util.Log.e("OpenLessNative", "failed to load openless_lib", error)
        }
    }

    @JvmStatic external fun nativeStartDictation()

    @JvmStatic external fun nativeStartDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeStopDictation()

    @JvmStatic external fun nativeStopDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeCancelDictation()

    @JvmStatic external fun nativeSwitchStylePack()

    @JvmStatic external fun nativeOpenQaFromOverlay()

    @JvmStatic external fun nativeFinalizeQaFromOverlay()

    @JvmStatic external fun nativeGetOverlayTriggerMode(): String

    @JvmStatic external fun nativeCanDrawOverlays(context: android.content.Context): Boolean

    @JvmStatic external fun nativeShowOverlay(context: android.content.Context)

    @JvmStatic external fun nativeHideOverlay(context: android.content.Context)

    @JvmStatic external fun nativeIsOverlayVisible(): Boolean

    @JvmStatic external fun nativeNotifyOverlayPermissionChanged(context: android.content.Context)

    @JvmStatic external fun nativeNotifyOverlayDestroyed()

    /** ADB specialist kit (debug builds). Returns a single status line for OpenLessAdb logcat. */
    @JvmStatic external fun nativeAdbDumpLog(destPath: String, appFilesDir: String?): String

    @JvmStatic external fun nativeAdbSetCredential(
        account: String,
        value: String,
        provider: String?,
    ): String

    @JvmStatic external fun nativeAdbSetAsrProvider(provider: String): String

    @JvmStatic external fun nativeAdbSetLlmProvider(provider: String): String

    @JvmStatic external fun nativeAdbValidate(kind: String): String

    @JvmStatic external fun nativeAdbApplyCredsJson(path: String): String
}
