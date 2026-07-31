package com.openless.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.util.Log
import androidx.annotation.Keep
import java.io.File

/**
 * ADB specialist entry points (debug builds only).
 *
 * Example:
 * ```
 * adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_DUMP_LOG
 * ```
 */
@Keep
class OpenLessAdbDebugReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        val action = intent?.action
        if (action.isNullOrBlank()) {
            Log.w(TAG, "REJECTED empty action")
            return
        }
        if (!isDebuggable(context)) {
            Log.e(TAG, "REJECTED not debuggable action=$action")
            return
        }

        // Prefer a warm process so Rust vault / log paths are initialized.
        try {
            OpenLessNative // touch object init / loadLibrary
        } catch (error: Throwable) {
            Log.e(TAG, "REJECTED native bridge unavailable", error)
            return
        }

        when (action) {
            ACTION_DUMP_LOG -> dumpLog(context)
            ACTION_SET_CREDENTIAL -> {
                val account = intent.getStringExtra(EXTRA_ACCOUNT).orEmpty()
                val value = intent.getStringExtra(EXTRA_VALUE).orEmpty()
                val provider = intent.getStringExtra(EXTRA_PROVIDER)
                logNative(OpenLessNative.nativeAdbSetCredential(account, value, provider))
            }
            ACTION_SET_ASR_PROVIDER -> {
                val provider = intent.getStringExtra(EXTRA_PROVIDER).orEmpty()
                logNative(OpenLessNative.nativeAdbSetAsrProvider(provider))
            }
            ACTION_SET_LLM_PROVIDER -> {
                val provider = intent.getStringExtra(EXTRA_PROVIDER).orEmpty()
                logNative(OpenLessNative.nativeAdbSetLlmProvider(provider))
            }
            ACTION_VALIDATE -> {
                val kind = intent.getStringExtra(EXTRA_KIND).orEmpty()
                logNative(OpenLessNative.nativeAdbValidate(kind))
            }
            ACTION_APPLY_CREDS_JSON -> {
                val path = intent.getStringExtra(EXTRA_PATH).orEmpty()
                logNative(OpenLessNative.nativeAdbApplyCredsJson(path))
            }
            else -> Log.w(TAG, "REJECTED unknown action=$action")
        }
    }

    private fun dumpLog(context: Context) {
        val dir = context.getExternalFilesDir(null)
        if (dir == null) {
            Log.e(TAG, "DUMP_ERR external files dir unavailable")
            return
        }
        if (!dir.exists() && !dir.mkdirs()) {
            Log.e(TAG, "DUMP_ERR cannot create ${dir.absolutePath}")
            return
        }
        val dest = File(dir, EXPORT_FILE_NAME)
        logNative(
            OpenLessNative.nativeAdbDumpLog(
                dest.absolutePath,
                context.applicationContext.filesDir.absolutePath,
            ),
        )
    }

    private fun logNative(result: String?) {
        val line = result?.trim().orEmpty()
        if (line.isEmpty()) {
            Log.e(TAG, "NATIVE_ERR empty result")
            return
        }
        if (line.startsWith("DUMP_OK") ||
            line.startsWith("SET_OK") ||
            line.startsWith("APPLY_OK") ||
            line.startsWith("VALIDATE_OK")
        ) {
            Log.i(TAG, line)
        } else {
            Log.e(TAG, line)
        }
    }

    companion object {
        const val TAG = "OpenLessAdb"

        const val ACTION_DUMP_LOG = "com.openless.app.ADB_DUMP_LOG"
        const val ACTION_SET_CREDENTIAL = "com.openless.app.ADB_SET_CREDENTIAL"
        const val ACTION_SET_ASR_PROVIDER = "com.openless.app.ADB_SET_ASR_PROVIDER"
        const val ACTION_SET_LLM_PROVIDER = "com.openless.app.ADB_SET_LLM_PROVIDER"
        const val ACTION_VALIDATE = "com.openless.app.ADB_VALIDATE"
        const val ACTION_APPLY_CREDS_JSON = "com.openless.app.ADB_APPLY_CREDS_JSON"

        const val EXTRA_ACCOUNT = "account"
        const val EXTRA_VALUE = "value"
        const val EXTRA_PROVIDER = "provider"
        const val EXTRA_KIND = "kind"
        const val EXTRA_PATH = "path"

        const val EXPORT_FILE_NAME = "openless-adb-export.log"

        private fun isDebuggable(context: Context): Boolean {
            return (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        }
    }
}
