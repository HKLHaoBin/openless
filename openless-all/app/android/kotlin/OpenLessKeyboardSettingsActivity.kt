package com.openless.app

import android.app.Activity
import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Full-screen native settings window opened by long-pressing the OpenLess
 * logo in the keyboard panels. Keyboard-only preferences that don't need the
 * full WebView app live here. Framework only for now (vibration intensity
 * and duration) — more rows get appended to `content` in buildContent() as
 * they're added.
 */
class OpenLessKeyboardSettingsActivity : Activity() {
    private val prefs by lazy { getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE) }

    // The new migration flow captures only after the user has selected a SAF
    // destination. Keep only small UI state across that picker round trip; the
    // bundle itself never lives in an Activity field.
    private var pendingExportMode: String? = null
    private var pendingExportIncludeCredentials = false
    private var pendingExportPassword: String? = null
    private val requestCodeExportSave = 4201
    private val requestCodeImportOpen = 4202
    private val requestCodeLegacyImportOpen = 4203
    private val maxMigrationBundleBytes = OpenLessMigrationBundlePolicy.MAX_BUNDLE_BYTES
    private data class PendingExportState(
        val mode: String,
        val includeCredentials: Boolean,
        val password: String?,
    )
    private val englishUi by lazy {
        val locale = prefs.getString("locale", null) ?: resources.configuration.locales[0].toLanguageTag()
        !locale.startsWith("zh", ignoreCase = true)
    }

    private fun ui(zh: String, en: String) = if (englishUi) en else zh
    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    /** Same "which theme is the keyboard actually showing" logic as OpenLessImeService.isDarkTheme, so this screen matches whatever the user is looking at when they long-press the Logo to get here. */
    private val isDarkTheme: Boolean
        get() = when (prefs.getString("theme_mode", null)) {
            "light" -> false
            "dark" -> true
            else -> (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) != Configuration.UI_MODE_NIGHT_NO
        }

    private fun tone(dark: Int, light: Int): Int = if (isDarkTheme) dark else light

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val retained = lastNonConfigurationInstance as? PendingExportState
        if (retained != null) {
            pendingExportMode = retained.mode
            pendingExportIncludeCredentials = retained.includeCredentials
            pendingExportPassword = retained.password
        } else if (savedInstanceState != null) {
            pendingExportMode = savedInstanceState.getString("migrationExportMode")
            pendingExportIncludeCredentials = savedInstanceState.getBoolean("migrationExportIncludeCredentials", false)
            // Passwords are deliberately not persisted in saved state. If the
            // process was killed while the picker was open, ask again on return.
            pendingExportPassword = null
        }
        // Explicit rather than relying on targetSdk 36's implicit
        // edge-to-edge enforcement — guarantees the IME-inset listener in
        // buildContent() actually fires so the "云笔记提交" URL/Token fields
        // stay reachable once the keyboard covers part of the screen.
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(buildContent())
        // Some OEM ROMs only wire up the WindowInsets/IME dispatch chain
        // correctly once a WindowInsetsControllerCompat has actually been
        // instantiated for this window — never used for show()/hide() here,
        // just created as the standard companion call to
        // setDecorFitsSystemWindows(false) above.
        WindowCompat.getInsetsController(window, window.decorView)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString("migrationExportMode", pendingExportMode)
        outState.putBoolean("migrationExportIncludeCredentials", pendingExportIncludeCredentials)
    }

    @Suppress("DEPRECATION")
    override fun onRetainCustomNonConfigurationInstance(): Any? =
        pendingExportMode?.let {
            PendingExportState(it, pendingExportIncludeCredentials, pendingExportPassword)
        }

    private fun buildContent(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(tone(Color.rgb(30, 30, 30), Color.rgb(245, 245, 247)))
        }
        val header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(8), dp(16), dp(8))
        }
        header.addView(
            TextView(this).apply {
                text = "←"
                textSize = 22f
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
                gravity = Gravity.CENTER
                setPadding(dp(16), dp(8), dp(16), dp(8))
                isClickable = true
                setOnClickListener { finish() }
            },
        )
        header.addView(
            TextView(this).apply {
                text = ui("键盘设置", "Keyboard settings")
                textSize = 18f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            },
        )
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        val scroll = ScrollView(this)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(12), dp(20), dp(20))
        }
        scroll.addView(content, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        root.addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        // Height/raise are adjusted in their own bottom sheet (see
        // showKeyboardSizeDialog()), not docked under this form: the 1:1
        // footprint is as tall as the keyboard itself, so keeping it on this
        // page permanently left everything above it only a sliver to scroll
        // in. This row just names the current values and opens the sheet.
        content.addView(sectionLabel(ui("键盘外观", "Keyboard appearance")))
        val sizeSummary = TextView(this).apply {
            text = keyboardSizeSummary()
            textSize = 14f
            setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
        }
        val sizeRow = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(6), 0, dp(6))
            isClickable = true
            setOnClickListener { showKeyboardSizeDialog { sizeSummary.text = keyboardSizeSummary() } }
        }
        sizeRow.addView(
            TextView(this).apply {
                text = ui("键盘高度", "Keyboard height")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        sizeRow.addView(sizeSummary)
        sizeRow.addView(
            TextView(this).apply {
                text = "›"
                textSize = 18f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(dp(8), 0, 0, 0)
            },
        )
        content.addView(
            sizeRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        // Paired with onCreate()'s setDecorFitsSystemWindows(false): now that
        // the window draws edge-to-edge, this restores the padding the
        // system used to apply automatically (status bar / nav bar / cutouts)
        // AND — the actual point of going edge-to-edge here — adds the IME's
        // own height as bottom padding whenever it's taller than the nav bar,
        // so `scroll` (layout_weight=1) genuinely loses that much height
        // while the keyboard is up — targetSdk 36 otherwise neutralizes the
        // manifest's windowSoftInputMode="adjustResize" (the window no
        // longer physically shrinks on its own), which is exactly why the
        // 云笔记提交 URL/Token fields were unreachable once the keyboard
        // covered them. Shrinking the viewport alone isn't enough on its
        // own, though: if a field already had focus before the keyboard
        // finished animating in, nothing re-triggers ScrollView's normal
        // "bring the focused child into view" behavior on a pure padding
        // change (that behavior only fires at the moment focus is first
        // requested) — so once the inset is actually nonzero, explicitly
        // scroll whatever's currently focused into the new, smaller
        // viewport instead of leaving it wherever it happened to sit before.
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val imeBottom = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
            view.setPadding(systemBars.left, systemBars.top, systemBars.right, maxOf(systemBars.bottom, imeBottom))
            if (imeBottom > 0) {
                val focused = view.findFocus()
                if (focused != null) {
                    scroll.post {
                        val rect = android.graphics.Rect()
                        focused.getDrawingRect(rect)
                        content.offsetDescendantRectToMyCoords(focused, rect)
                        scroll.smoothScrollTo(0, rect.bottom - scroll.height + dp(16))
                    }
                }
            }
            insets
        }

        // Gates the "进程重启统计（今天）" section further down (see its own
        // guard) — off by default, since those counters are only meaningful
        // for diagnosing a specific problem, not everyday reading. Read once
        // here, up top, rather than re-reading prefs at the exact point it's
        // used, so this row and the section it controls can never disagree
        // within a single render of this page.
        content.addView(sectionLabel(ui("主设置", "Main settings")))
        val debugFeaturesEnabled = prefs.getBoolean("key_debug_features_enabled", false)
        val debugRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        debugRow.addView(
            TextView(this).apply {
                text = ui("调试功能", "Debug features")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        debugRow.addView(
            Switch(this).apply {
                isChecked = debugFeaturesEnabled
                setOnCheckedChangeListener { _, checked ->
                    prefs.edit().putBoolean("key_debug_features_enabled", checked).apply()
                    // The restart-stats section's own visibility is decided
                    // once, above, when this page was built — rebuild it so
                    // toggling here shows/hides it immediately instead of
                    // only taking effect the next time this page opens.
                    setContentView(buildContent())
                }
            },
        )
        content.addView(
            debugRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        content.addView(sectionLabel(ui("震动反馈", "Haptic feedback")))

        val enabledRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        enabledRow.addView(
            TextView(this).apply {
                text = ui("按键震动", "Key vibration")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        enabledRow.addView(
            Switch(this).apply {
                isChecked = prefs.getBoolean("key_haptic_enabled", true)
                setOnCheckedChangeListener { _, checked -> prefs.edit().putBoolean("key_haptic_enabled", checked).apply() }
            },
        )
        content.addView(
            enabledRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        content.addView(sectionLabel(ui("英文键盘", "English keyboard")))
        val englishSuggestionsRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        englishSuggestionsRow.addView(
            TextView(this).apply {
                text = ui("英文单词提示", "English word suggestions")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        englishSuggestionsRow.addView(
            Switch(this).apply {
                isChecked = prefs.getBoolean("english_suggestions_enabled", true)
                setOnCheckedChangeListener { _, checked -> prefs.edit().putBoolean("english_suggestions_enabled", checked).apply() }
            },
        )
        content.addView(
            englishSuggestionsRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        // Amplitude's 255 ceiling is Android's own VibrationEffect max, not a
        // choice made here — the hardware/API can't go any stronger than
        // that regardless of what this slider allows. Duration's ceiling
        // started at 500ms, halved to 250, then halved again to 125 so the
        // same slider width covers a quarter of the original range, for
        // finer-grained adjustment.
        var currentAmplitude = prefs.getInt("key_haptic_amplitude", 55).coerceIn(1, 255)
        var currentDurationMs = prefs.getLong("key_haptic_duration_ms", 12L).toInt().coerceIn(1, 125)
        // No separate test button — letting go of either slider fires one
        // vibration with the values as they now stand, so adjusting and
        // feeling the result is a single motion.
        content.addView(
            sliderRow(
                label = ui("震动强度（系统上限）", "Intensity (platform ceiling)"),
                min = 1,
                max = 255,
                current = currentAmplitude,
                onChange = { value ->
                    currentAmplitude = value
                    prefs.edit().putInt("key_haptic_amplitude", value).apply()
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )
        content.addView(
            sliderRow(
                label = ui("震动时长", "Duration"),
                min = 1,
                max = 125,
                current = currentDurationMs,
                onChange = { value ->
                    currentDurationMs = value
                    prefs.edit().putLong("key_haptic_duration_ms", value.toLong()).apply()
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )

        content.addView(sectionLabel(ui("后台运行", "Background")))
        // OEM background-task killers (observed on-device: this app's own
        // settings Activity gets reclaimed by the system 16+ times/day even
        // with a foreground service running) largely ignore that
        // protection but do respect the standard "ignore battery
        // optimizations" exemption — offering a direct link to it here is
        // the most effective single thing a user can do about the restart
        // counts below. isIgnoringBatteryOptimizations() re-reads live each
        // time this screen builds, so returning here after granting it in
        // system settings shows the up-to-date state without extra wiring.
        val powerManager = getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
        if (powerManager.isIgnoringBatteryOptimizations(packageName)) {
            content.addView(
                TextView(this).apply {
                    text = ui("已加入电池优化白名单", "Already exempt from battery optimization")
                    textSize = 13f
                    setTextColor(tone(Color.rgb(134, 239, 172), Color.rgb(21, 128, 61)))
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(14)
                },
            )
        } else {
            content.addView(
                TextView(this).apply {
                    text = ui(
                        "系统电量管理可能频繁回收键盘的后台进程，导致设置页偶尔黑屏或响应变慢。加入电池优化白名单可以减少这种情况——效果因系统而异。",
                        "The system's battery manager may repeatedly reclaim the keyboard's background process, occasionally causing a black settings screen or slow responses. Exempting it from battery optimization can reduce this — effectiveness varies by device.",
                    )
                    textSize = 13f
                    setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(8)
                },
            )
            content.addView(
                TextView(this).apply {
                    text = ui("去允许后台活动 →", "Allow background activity →")
                    textSize = 15f
                    setTypeface(typeface, android.graphics.Typeface.BOLD)
                    setTextColor(tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110)))
                    isClickable = true
                    setOnClickListener {
                        val direct = android.content.Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                            data = android.net.Uri.parse("package:$packageName")
                        }
                        val fallback = android.content.Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                            data = android.net.Uri.parse("package:$packageName")
                        }
                        runCatching { startActivity(direct) }
                            // A handful of heavily customized OEM systems
                            // block or silently no-op this specific system
                            // intent — the general app-details screen at
                            // least lands the user in the right area, one
                            // tap further from the actual toggle, instead
                            // of nothing happening on tap.
                            .onFailure { runCatching { startActivity(fallback) } }
                    }
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(14)
                },
            )
        }

        val monospace = android.graphics.Typeface.MONOSPACE
        // Diagnostic-only — hidden unless "调试功能" above is on (see that
        // switch's own comment). These counters help debug a specific
        // problem; they're noise for everyday reading otherwise.
        if (debugFeaturesEnabled) {
            content.addView(sectionLabel(ui("进程重启统计（今天）", "Process restarts (today)")))
            // Short, purposefully un-translated keys (not meant to be pretty —
            // meant to be pasted into a screenshot and read back verbatim).
            // All reset to 0 whenever OpenLessBuildInfo.VERSION changes (see
            // OpenLessApplication.resetRestartStatsOnVersionBump()), so these
            // are always "since this build was installed", not lifetime totals:
            //   main/access  - raw restarts of the main / :accessibility process
            //   sticky       - OpenLessRuntimeService.onStartCommand() got a
            //                  null Intent: Android's own restart-after-death
            //                  signal for a START_STICKY service, the strongest
            //                  evidence the whole process was actually killed
            //   warmup       - OpenLessBackendWarmupActivity.ensureBackendReady()
            //                  found the backend not registered and launched
            //                  the warmup Activity
            //   mictap       - user tapped the mic and toggleDictation() found
            //                  the backend not ready (the user-visible symptom)
            //   actkill      - OpenLessBackendWarmupActivity.onDestroy() fired,
            //                  total across all reasons below (doesn't
            //                  necessarily mean the process itself died)
            //   actkill_config - onDestroy() from a configuration change
            //                    (rotation/density/locale) — expected, harmless
            //   actkill_finishing - isFinishing was true (unexpected; this
            //                       Activity never calls finish() on itself
            //                       deliberately)
            //   actkill_os     - none of the above: the only sub-category that
            //                    is actually the system reclaiming this task
            //   rtexit       - Tauri's RunEvent::Exit actually fired despite
            //                  ExitRequested being prevented (see
            //                  mobile_runtime.rs) — should stay at 0 if that fix
            //                  is holding
            //   unclean      - previous main-process session never reached
            //                  OpenLessImeService.onDestroy() (best-effort
            //                  crash/force-stop signal, can't tell those apart)
            // Chinese gloss for each key — just enough to read at a glance
            // without cross-referencing the doc comment above.
            val restartCategories = listOf(
                Triple(OpenLessProcessRestartStats.MAIN, "main", "主进程"),
                Triple(OpenLessProcessRestartStats.ACCESSIBILITY, "access", "无障碍进程"),
                Triple("sticky", "sticky", "系统杀后恢复"),
                Triple("warmup", "warmup", "后端唤醒"),
                Triple("mictap", "mictap", "点击时未就绪"),
                Triple("actkill", "actkill", "界面被回收(合计)"),
                Triple("actkill_config", "  ├config", "· 配置变化(无害)"),
                Triple("actkill_finishing", "  ├finish", "· isFinishing(异常)"),
                Triple("actkill_os", "  └os", "· 真正被系统回收"),
                Triple("rtexit", "rtexit", "后端异常退出"),
                Triple("unclean", "unclean", "上次异常退出"),
                Triple("heartbeat", "heartbeat", "心跳自愈"),
                Triple("stuckwindow", "stuckwindow", "设置窗口卡死自重启"),
            )
            for ((key, label, gloss) in restartCategories) {
                content.addView(
                    TextView(this).apply {
                        val count = OpenLessProcessRestartStats(this@OpenLessKeyboardSettingsActivity, key).today()
                        text = label.padEnd(10) + count.toString().padEnd(4) + gloss
                        textSize = 13f
                        typeface = monospace
                        setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                    },
                )
            }
            content.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(14)))
        }

        // "个人偏好数据": read-only counters for the two local, on-device-only
        // learning stores that make repeated input steadily rank better
        // (笔画/拼音 both benefit — see each row's own comment). No toggle
        // here — matches this section's existing convention of pure
        // display; the underlying preferences (strokeUsageEnabled etc.)
        // live in the app's own WebView settings, not this native page.
        content.addView(sectionLabel(ui("个人偏好数据", "Personal preference data")))
        // 笔画输入的个人调频数据：记录"这个笔画码你选过哪个字"，越用越靠前
        // 排序，不影响词库本身，也不会同步到云端。
        val personalFrequency = StrokeUserFrequency(this)
        content.addView(
            TextView(this).apply {
                text = ui(
                    "笔画调频：已记录 ${personalFrequency.size()} / ${personalFrequency.capacity()} 条",
                    "Stroke ranking: ${personalFrequency.size()} / ${personalFrequency.capacity()} entries recorded",
                )
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
            },
        )
        // 简拼优选：不是每次输入都记一条，是"连续两次直接打拼音上屏"的组合
        // （比如先打 zg 选中国，紧接着打 rm 选人民）达到 3 次后才计入这里——
        // 见 LitePinyinLearnedPhrases 的文档注释。这里只显示已经达标、正在
        // 生效的组合数，未达标的候选不计入（避免这个数字本身产生误导）。
        val learnedPhrases = LitePinyinLearnedPhrases(this)
        content.addView(
            TextView(this).apply {
                text = ui(
                    "简拼优选：已生效 ${learnedPhrases.promotedCount()} 条",
                    "Pinyin combo learning: ${learnedPhrases.promotedCount()} promoted",
                )
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                setPadding(0, 0, 0, dp(14))
            },
        )

        // 话筒右划进入"云笔记"：录音经 LLM 整理后的文字（跟正常上屏用的是
        // 同一条润色流程——见 OpenLessImeService.handleImeTextReady()）会以
        // JSON POST 到这里配置的地址，不插入任何输入框，也不在本机留存。
        // 两项都填了才会真的提交——留空时只会在状态栏提示去设置里补上，不会
        // 静默失败。
        content.addView(sectionLabel(ui("云笔记提交", "Cloud notes webhook")))
        content.addView(
            TextView(this).apply {
                text = ui(
                    "话筒右划进入「云笔记」模式：录音经过整理后的文字会提交到下面的地址，不插入输入框，也不保存在本机。",
                    "Swipe the mic right to enter Cloud notes mode: the polished transcript is POSTed to the address below instead of being inserted — nothing is kept on this device either.",
                )
                textSize = 12f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(0, 0, 0, dp(10))
            },
        )
        content.addView(
            textFieldRow(
                label = ui("提交地址", "Submit URL"),
                hint = "https://example.com/capture_ingest.php",
                initial = prefs.getString("key_cloud_note_webhook_url", "") ?: "",
                onChange = { prefs.edit().putString("key_cloud_note_webhook_url", it).apply() },
            ),
        )
        content.addView(
            textFieldRow(
                label = "Token",
                hint = ui("输入法专用 Token", "IME-only token"),
                initial = prefs.getString("key_cloud_note_webhook_token", "") ?: "",
                isSecret = true,
                onChange = { prefs.edit().putString("key_cloud_note_webhook_token", it).apply() },
            ),
        )

        content.addView(sectionLabel(ui("本地迁移 / 分享", "Local migration / sharing")))
        content.addView(
            TextView(this).apply {
                text = ui(
                    "默认分享模式会脱敏并排除所有凭据。个人迁移如需携带凭据，必须使用密码保护；系统权限、OAuth 会话、设备路径和运行时状态不会迁移。导入前会显示文件摘要，并自动创建本地备份。",
                    "Share mode is redacted and excludes all credentials. Personal migration credentials require a password. System permissions, OAuth sessions, device paths, and runtime state never migrate. Import shows a summary first and creates a local backup before applying.",
                )
                textSize = 12f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(0, 0, 0, dp(10))
            },
        )
        val migrationRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        migrationRow.addView(
            TextView(this).apply {
                text = ui("导出迁移文件 →", "Export migration bundle →")
                textSize = 15f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110)))
                isClickable = true
                setOnClickListener { showMigrationExportDialog() }
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        migrationRow.addView(
            TextView(this).apply {
                text = ui("导入迁移文件 →", "Import migration bundle →")
                textSize = 15f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110)))
                isClickable = true
                setOnClickListener { launchMigrationImportPicker() }
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        content.addView(
            migrationRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(16)
            },
        )

        content.addView(
            TextView(this).apply {
                text = ui("导入旧版 JSON 配置 →", "Import legacy JSON settings →")
                textSize = 13f
                setTextColor(tone(Color.rgb(170, 170, 170), Color.rgb(90, 90, 95)))
                isClickable = true
                setOnClickListener { launchLegacyImportPicker() }
                setPadding(0, 0, 0, dp(16))
            },
        )

        // Legacy v1/v2 JSON import remains available only through the old
        // compatibility object and is intentionally not used by new export.
        content.addView(
            TextView(this).apply {
                text = ui("旧版 JSON 导入仍可由兼容层处理；新导出始终使用带版本和校验的 .omb 文件。", "Legacy JSON import remains available to the compatibility layer; new exports always use versioned, digested .omb bundles.")
                textSize = 11f
                setTextColor(tone(Color.rgb(120, 120, 120), Color.rgb(150, 150, 155)))
            },
        )

        // build_first_seen_wall_time is written by OpenLessApplication's
        // resetRestartStatsOnVersionBump() at the exact moment it last
        // zeroed the restart-cause counters below — i.e. "counting since
        // when" for whatever counts are on screen right now, so a
        // screenshot of this page carries both together.
        val buildFirstSeenAt = getSharedPreferences("openless_runtime", Context.MODE_PRIVATE)
            .getLong("build_first_seen_wall_time", 0L)
        val installedAtText = if (buildFirstSeenAt > 0L) {
            android.text.format.DateFormat.format("yyyy-MM-dd HH:mm", buildFirstSeenAt)
        } else {
            "?"
        }
        content.addView(
            TextView(this).apply {
                text = "${ui("构建版本", "Build")} ${OpenLessBuildInfo.VERSION}  ${ui("安装于", "installed")} $installedAtText"
                textSize = 11f
                typeface = monospace
                setTextColor(tone(Color.rgb(120, 120, 120), Color.rgb(150, 150, 155)))
            },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(16)
            },
        )

        return root
    }

    private fun showMigrationExportDialog() {
        val modes = arrayOf(
            ui("分享（脱敏，不带凭据）", "Share (redacted, no credentials)"),
            ui("个人迁移（可选密码保护凭据）", "Personal migration (optional password-protected credentials)"),
        )
        android.app.AlertDialog.Builder(this)
            .setTitle(ui("选择导出模式", "Choose export mode"))
            .setItems(modes) { _, which ->
                if (which == 0) {
                    launchMigrationExportPicker("share_redacted", false, null)
                } else {
                    showMigrationPasswordDialog { password ->
                        launchMigrationExportPicker("personal_migration", true, password)
                    }
                }
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    private fun showMigrationPasswordDialog(onConfirmed: (String) -> Unit) {
        val field = EditText(this).apply {
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
            hint = ui("至少输入一个密码字符", "Enter a password")
        }
        val container = FrameLayout(this).apply {
            setPadding(dp(24), 0, dp(24), 0)
            addView(field, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        android.app.AlertDialog.Builder(this)
            .setTitle(ui("保护迁移凭据", "Protect migration credentials"))
            .setMessage(ui("密码不会写入文件名、摘要或日志。请在目标设备导入时输入相同密码。", "The password is never written to the filename, summary, or logs. Enter the same password on the destination device."))
            .setView(container)
            .setPositiveButton(ui("继续选择文件", "Choose destination")) { _, _ ->
                val password = field.text.toString()
                if (password.isBlank()) {
                    Toast.makeText(this, ui("密码不能为空", "Password cannot be empty"), Toast.LENGTH_SHORT).show()
                } else {
                    onConfirmed(password)
                }
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    private fun launchMigrationExportPicker(mode: String, includeCredentials: Boolean, password: String?) {
        if (!OpenLessMigrationBundlePolicy.validateExport(mode, includeCredentials, password)) {
            Toast.makeText(this, ui("导出选项无效", "Invalid export options"), Toast.LENGTH_SHORT).show()
            return
        }
        pendingExportMode = mode
        pendingExportIncludeCredentials = includeCredentials
        pendingExportPassword = password
        val timestamp = android.text.format.DateFormat.format("yyyyMMdd-HHmm", System.currentTimeMillis())
        val intent = android.content.Intent(android.content.Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(android.content.Intent.CATEGORY_OPENABLE)
            type = "application/octet-stream"
            putExtra(android.content.Intent.EXTRA_TITLE, "openless-migration-$timestamp.omb")
        }
        runCatching { startActivityForResult(intent, requestCodeExportSave) }.onFailure {
            clearPendingExport()
            Toast.makeText(this, ui("找不到文件管理器", "No file manager available"), Toast.LENGTH_LONG).show()
        }
    }

    private fun clearPendingExport() {
        pendingExportMode = null
        pendingExportIncludeCredentials = false
        pendingExportPassword = null
    }

    private fun launchMigrationImportPicker() {
        val intent = android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(android.content.Intent.CATEGORY_OPENABLE)
            type = "application/octet-stream"
        }
        runCatching { startActivityForResult(intent, requestCodeImportOpen) }
            .onFailure { Toast.makeText(this, ui("找不到文件选择器", "No file picker available"), Toast.LENGTH_SHORT).show() }
    }

    private fun launchLegacyImportPicker() {
        val intent = android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(android.content.Intent.CATEGORY_OPENABLE)
            type = "application/json"
        }
        runCatching { startActivityForResult(intent, requestCodeLegacyImportOpen) }
            .onFailure { Toast.makeText(this, ui("找不到文件选择器", "No file picker available"), Toast.LENGTH_SHORT).show() }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK || data == null) {
            if (requestCode == requestCodeExportSave) clearPendingExport()
            return
        }
        when (requestCode) {
            requestCodeExportSave -> {
                val uri = data.data
                val mode = pendingExportMode
                val includeCredentials = pendingExportIncludeCredentials
                val password = pendingExportPassword.orEmpty()
                clearPendingExport()
                if (uri == null || mode == null) return
                if (includeCredentials && password.isEmpty()) {
                    showMigrationPasswordDialog { restoredPassword -> performMigrationExport(uri, mode, true, restoredPassword) }
                } else {
                    performMigrationExport(uri, mode, includeCredentials, password)
                }
            }
            requestCodeImportOpen -> {
                val uri = data.data ?: return
                Thread {
                    val result = runCatching {
                        val bytes = OpenLessContentReader.readBytes(this, uri.toString(), maxMigrationBundleBytes)
                            ?: error("read selected document failed")
                        val bundleBase64 = android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)
                        val envelope = org.json.JSONObject(OpenLessNative.nativeInspectLocalMigrationBundle(bundleBase64))
                        check(envelope.optBoolean("ok")) { envelope.optString("error", "invalid migration bundle") }
                        bundleBase64 to envelope.getJSONObject("payload")
                    }
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        result.onSuccess { (bundleBase64, inspection) -> showMigrationImportSummary(bundleBase64, inspection) }
                            .onFailure { error ->
                                Toast.makeText(this, ui("读取或校验失败：${error.message}", "Read or validation failed: ${error.message}"), Toast.LENGTH_LONG).show()
                            }
                    }
                }.start()
            }
            requestCodeLegacyImportOpen -> {
                val uri = data.data ?: return
                Thread {
                    val json = OpenLessContentReader.readBytes(this, uri.toString(), 4 * 1024 * 1024)
                        ?.toString(Charsets.UTF_8)
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        if (json.isNullOrBlank()) {
                            Toast.makeText(this, ui("读取旧版配置失败或文件超过 4 MiB", "Read failed or legacy file exceeds 4 MiB"), Toast.LENGTH_SHORT).show()
                        } else {
                            showImportCategoryDialog(json)
                        }
                    }
                }.start()
            }
        }
    }

    private fun showImportCategoryDialog(json: String) {
        val present = OpenLessSettingsExport.categoriesPresent(json)
        if (present.isEmpty()) {
            Toast.makeText(this, ui("这不是一个有效的配置文件", "Not a valid settings file"), Toast.LENGTH_SHORT).show()
            return
        }
        val categories = present.toList()
        val labels = categories.map { ui(it.labelZh, it.labelEn) }.toTypedArray()
        val checked = BooleanArray(categories.size) { true }
        android.app.AlertDialog.Builder(this)
            .setTitle(ui("选择要导入的内容", "Choose what to import"))
            .setMultiChoiceItems(labels, checked) { _, which, isChecked -> checked[which] = isChecked }
            .setPositiveButton(ui("导入", "Import")) { _, _ ->
                val selected = categories.filterIndexed { index, _ -> checked[index] }.toSet()
                if (selected.isEmpty()) {
                    Toast.makeText(this, ui("没有选择任何内容", "Nothing selected"), Toast.LENGTH_SHORT).show()
                    return@setPositiveButton
                }
                Thread {
                    val imported = runCatching { OpenLessSettingsExport.import(this, json, selected) }
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        val result = imported.getOrNull()
                        val message = if (result == null) {
                            ui("配置文件无效，导入失败", "Invalid settings file; import failed")
                        } else {
                            buildString {
                                append(ui("已导入 ${result.applied.size} 项", "Imported ${result.applied.size} categories"))
                                for ((category, reason) in result.errors) {
                                    append('\n').append(ui(category.labelZh, category.labelEn)).append(": ").append(reason)
                                }
                            }
                        }
                        android.app.AlertDialog.Builder(this)
                            .setMessage(message)
                            .setPositiveButton(android.R.string.ok, null)
                            .show()
                        setContentView(buildContent())
                    }
                }.start()
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    private fun performMigrationExport(
        uri: android.net.Uri,
        mode: String,
        includeCredentials: Boolean,
        password: String,
    ) {
        Thread {
            val result = runCatching {
                val envelope = org.json.JSONObject(
                    OpenLessNative.nativeExportLocalMigrationBundle(
                        mode,
                        includeCredentials,
                        password,
                        OpenLessSettingsExport.exportPortable(this),
                    ),
                )
                check(envelope.optBoolean("ok")) { envelope.optString("error", "export failed") }
                val payload = envelope.getJSONObject("payload")
                val bytes = android.util.Base64.decode(payload.getString("bundleBase64"), android.util.Base64.DEFAULT)
                check(bytes.size <= maxMigrationBundleBytes) { "bundle exceeds size limit" }
                check(OpenLessContentWriter.writeBytes(this, uri.toString(), bytes)) { "write selected document failed" }
                payload.getJSONObject("inspection")
            }
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.onSuccess { inspection ->
                    Toast.makeText(this, ui("已导出迁移文件（${inspection.optInt("documentCount")} 项）", "Migration bundle exported (${inspection.optInt("documentCount")} documents)"), Toast.LENGTH_LONG).show()
                }.onFailure { error ->
                    Toast.makeText(this, ui("导出失败：${error.message}", "Export failed: ${error.message}"), Toast.LENGTH_LONG).show()
                }
            }
        }.start()
    }

    private fun migrationInspectionText(inspection: org.json.JSONObject): String = buildString {
        append(ui("格式：", "Format: ")).append(inspection.optString("format")).append('\n')
        append(ui("版本：", "Version: ")).append(inspection.optInt("formatVersion")).append('\n')
        append(ui("文档：", "Documents: ")).append(inspection.optInt("documentCount")).append('\n')
        append(ui("渠道：", "Channels: ")).append(inspection.optJSONObject("sectionCounts")?.optInt("channels") ?: 0).append('\n')
        append(ui("词典 / 纠错 / 风格包：", "Dictionary / corrections / style packs: "))
            .append(inspection.optJSONObject("sectionCounts")?.optInt("dictionary") ?: 0).append(" / ")
            .append(inspection.optJSONObject("sectionCounts")?.optInt("corrections") ?: 0).append(" / ")
            .append(inspection.optJSONObject("sectionCounts")?.optInt("stylePacks") ?: 0).append('\n')
        append(ui("凭据：", "Credentials: ")).append(
            if (inspection.optBoolean("credentialsEncrypted")) ui("已加密", "password protected") else ui("未携带", "not included"),
        ).append('\n')
        append(ui("系统权限不会迁移，目标设备需重新授权。", "System permissions do not migrate; authorize them again on the destination device."))
    }

    private fun showMigrationImportSummary(bundleBase64: String, inspection: org.json.JSONObject) {
        android.app.AlertDialog.Builder(this)
            .setTitle(ui("确认迁移文件", "Review migration bundle"))
            .setMessage(migrationInspectionText(inspection))
            .setPositiveButton(ui("选择合并 / 替换", "Choose merge / replace")) { _, _ ->
                val encrypted = inspection.optBoolean("credentialsEncrypted")
                if (encrypted) {
                    showMigrationPasswordDialog { password -> showMigrationRestoreMode(bundleBase64, password) }
                } else {
                    showMigrationRestoreMode(bundleBase64, "")
                }
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    private fun showMigrationRestoreMode(bundleBase64: String, password: String) {
        Thread {
            val previewResult = runCatching {
                val response = org.json.JSONObject(
                    OpenLessNative.nativePreviewLocalMigrationBundle(
                        bundleBase64,
                        password,
                        OpenLessSettingsExport.exportPortable(this@OpenLessKeyboardSettingsActivity),
                    ),
                )
                check(response.optBoolean("ok")) { response.optString("error", "preview failed") }
                response.getJSONObject("payload")
            }
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                previewResult.onSuccess { preview ->
                    val conflicts = preview.getJSONArray("conflicts")
                    val expectedGeneration = preview.optString("observedGeneration")
                    chooseMigrationConflicts(bundleBase64, password, conflicts, 0, org.json.JSONArray()) {
                        showMigrationRestoreModeAfterConflicts(bundleBase64, password, expectedGeneration, conflicts, it)
                    }
                }.onFailure { error ->
                    Toast.makeText(this, ui("预览失败：${error.message}", "Preview failed: ${error.message}"), Toast.LENGTH_LONG).show()
                }
            }
        }.start()
    }

    private fun chooseMigrationConflicts(
        bundleBase64: String,
        password: String,
        conflicts: org.json.JSONArray,
        index: Int,
        choices: org.json.JSONArray,
        onComplete: (org.json.JSONArray) -> Unit,
    ) {
        if (index >= conflicts.length()) {
            onComplete(choices)
            return
        }
        val conflict = conflicts.getJSONObject(index)
        val kind = conflict.optString("kind")
        val reason = conflict.optString("reason")
        val credential = if (conflict.optBoolean("isCredentialUnit")) ui("（凭据关联渠道）", " (credential-linked channel)") else ""
        var selected: String? = null
        val dialog = android.app.AlertDialog.Builder(this)
            .setTitle(ui("选择冲突 ${index + 1}/${conflicts.length()}", "Choose conflict ${index + 1}/${conflicts.length()}"))
            .setMessage("$kind / $reason$credential")
            .setSingleChoiceItems(arrayOf(ui("保留当前设备", "Keep this device"), ui("使用迁移文件", "Use migration file")), -1) { _, which ->
                selected = if (which == 0) "local" else "remote"
            }
            .setPositiveButton(ui("继续", "Continue"), null)
            .setNegativeButton(ui("取消", "Cancel"), null)
            .create()
        dialog.setOnShowListener {
            dialog.getButton(android.app.AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                val side = selected
                if (side == null) {
                    Toast.makeText(this, ui("必须选择一侧", "Choose one side"), Toast.LENGTH_SHORT).show()
                    return@setOnClickListener
                }
                choices.put(org.json.JSONObject().put("conflictId", conflict.getString("conflictId")).put("side", side))
                dialog.dismiss()
                chooseMigrationConflicts(bundleBase64, password, conflicts, index + 1, choices, onComplete)
            }
        }
        dialog.show()
    }

    private fun portableConflictSide(
        conflicts: org.json.JSONArray,
        choices: org.json.JSONArray,
    ): String? {
        for (i in 0 until conflicts.length()) {
            val conflict = conflicts.getJSONObject(i)
            if (conflict.optString("documentId") != "androidPortableSettings") continue
            val conflictId = conflict.optString("conflictId")
            for (j in 0 until choices.length()) {
                val choice = choices.getJSONObject(j)
                if (choice.optString("conflictId") == conflictId) {
                    return choice.optString("side")
                }
            }
        }
        return null
    }

    private fun showMigrationRestoreModeAfterConflicts(
        bundleBase64: String,
        password: String,
        expectedGeneration: String,
        conflicts: org.json.JSONArray,
        choices: org.json.JSONArray,
    ) {
        val modes = arrayOf(ui("合并", "Merge"), ui("替换", "Replace"))
        android.app.AlertDialog.Builder(this)
            .setTitle(ui("导入方式", "Restore mode"))
            .setItems(modes) { _, which ->
                val mode = if (which == 0) "merge" else "replace"
                Thread {
                    val applyPortable = mode == "replace" || portableConflictSide(conflicts, choices) != "local"
                    val mergePortableLearning = mode == "merge" && applyPortable
                    val result = runCatching {
                        val response = org.json.JSONObject(
                            OpenLessNative.nativeImportLocalMigrationBundle(
                                bundleBase64,
                                password,
                                mode,
                                choices.toString(),
                                expectedGeneration,
                                OpenLessSettingsExport.exportPortable(this@OpenLessKeyboardSettingsActivity),
                            ),
                        )
                        check(response.optBoolean("ok")) { response.optString("error", "import failed") }
                        val payload = response.getJSONObject("payload")
                        val portableResponse = org.json.JSONObject(
                            OpenLessNative.nativeExtractLocalMigrationAndroidPreferences(bundleBase64, password),
                        )
                        check(portableResponse.optBoolean("ok")) { portableResponse.optString("error", "portable settings read failed") }
                        val portable = portableResponse.getJSONObject("payload").opt("preferences")
                        if (portable !is org.json.JSONObject && portable != org.json.JSONObject.NULL) {
                            error("invalid portable Android settings")
                        }
                        Triple(
                            payload,
                            (portable as? org.json.JSONObject)?.toString(),
                            applyPortable to mergePortableLearning,
                        )
                    }
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        result.onSuccess { (_, portableJson, portableOptions) ->
                            runCatching {
                                if (portableOptions.first) {
                                    portableJson?.let {
                                        OpenLessSettingsExport.importPortable(this, it, portableOptions.second)
                                    }
                                }
                            }.onSuccess {
                                Toast.makeText(this, ui("导入完成，备份已保留", "Import complete; backup retained"), Toast.LENGTH_LONG).show()
                                setContentView(buildContent())
                            }.onFailure { error ->
                                Toast.makeText(this, ui("核心数据已导入，但 Android 偏好未应用：${error.message}", "Core data imported, but Android preferences were not applied: ${error.message}"), Toast.LENGTH_LONG).show()
                            }
                        }.onFailure { error ->
                            Toast.makeText(this, ui("导入未完成：${error.message}", "Import not completed: ${error.message}"), Toast.LENGTH_LONG).show()
                        }
                    }
                }.start()
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    /** Fires a one-shot vibration with the sliders' current (already-saved) values, so a change is felt immediately. */
    private fun fireTestVibration(amplitude: Int, durationMs: Int) {
        runCatching {
            val vibrator = if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) {
                (getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as android.os.VibratorManager).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Context.VIBRATOR_SERVICE) as android.os.Vibrator
            }
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
                vibrator.vibrate(android.os.VibrationEffect.createOneShot(durationMs.toLong(), amplitude))
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(durationMs.toLong())
            }
        }
    }

    private fun sectionLabel(text: String): View = TextView(this).apply {
        this.text = text
        textSize = 12f
        setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
        setPadding(0, 0, 0, dp(8))
    }

    private fun keyboardSizeSummary(): String {
        val heightDp = OpenLessImeService.panelHeightDp(this)
        val raiseDp = OpenLessImeService.raiseHeightDp(this)
        return ui("按键区 ${heightDp}dp · 抬高 ${raiseDp}dp", "Keys ${heightDp}dp · raise ${raiseDp}dp")
    }

    /**
     * Bottom sheet for the key-area height and raise: two sliders over a
     * 1:1 footprint that sits flush with the bottom of the screen — exactly
     * where the real keyboard will. Values save as they change, same as
     * every other row on this page; onClosed lets the caller refresh
     * whatever it shows of them.
     */
    private fun showKeyboardSizeDialog(onClosed: () -> Unit) {
        var heightDp = OpenLessImeService.panelHeightDp(this)
        var raiseDp = OpenLessImeService.raiseHeightDp(this)
        val footprint = buildKeyboardFootprintPreview()
        fun save() {
            prefs.edit()
                .putInt(OpenLessImeService.PREF_KEYBOARD_HEIGHT_DP, heightDp)
                .putInt(OpenLessImeService.PREF_KEYBOARD_RAISE_DP, raiseDp)
                .apply()
            footprint.setSizes(heightDp, raiseDp)
        }
        val heightSlider = steppedSliderRow(
            label = ui("按键区高度", "Key area height"),
            min = OpenLessImeService.MIN_KEYBOARD_HEIGHT_DP,
            max = OpenLessImeService.MAX_KEYBOARD_HEIGHT_DP,
            current = heightDp,
            onChange = { value ->
                heightDp = value
                save()
            },
        )
        val raiseSlider = steppedSliderRow(
            label = ui("底部抬高", "Raise from bottom"),
            min = OpenLessImeService.MIN_KEYBOARD_RAISE_DP,
            max = OpenLessImeService.MAX_KEYBOARD_RAISE_DP,
            current = raiseDp,
            onChange = { value ->
                raiseDp = value
                save()
            },
        )

        val dialog = android.app.Dialog(this)
        val linkColor = tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110))
        val titleRow = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 0, 0, dp(12))
        }
        titleRow.addView(
            TextView(this).apply {
                text = ui("键盘高度", "Keyboard height")
                textSize = 17f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        titleRow.addView(
            TextView(this).apply {
                text = ui("恢复默认", "Reset")
                textSize = 14f
                setTextColor(linkColor)
                setPadding(dp(10), dp(6), dp(10), dp(6))
                isClickable = true
                setOnClickListener {
                    heightSlider.set(OpenLessImeService.DEFAULT_KEYBOARD_HEIGHT_DP)
                    raiseSlider.set(OpenLessImeService.DEFAULT_KEYBOARD_RAISE_DP)
                }
            },
        )
        titleRow.addView(
            TextView(this).apply {
                text = ui("完成", "Done")
                textSize = 14f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(linkColor)
                setPadding(dp(10), dp(6), 0, dp(6))
                isClickable = true
                setOnClickListener { dialog.dismiss() }
            },
        )
        val controls = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), dp(2))
            background = android.graphics.drawable.GradientDrawable().apply {
                setColor(tone(Color.rgb(30, 30, 30), Color.rgb(245, 245, 247)))
                val radius = dp(16).toFloat()
                cornerRadii = floatArrayOf(radius, radius, radius, radius, 0f, 0f, 0f, 0f)
            }
            addView(
                View(this@OpenLessKeyboardSettingsActivity).apply {
                    background = android.graphics.drawable.GradientDrawable().apply {
                        setColor(tone(Color.rgb(90, 90, 90), Color.rgb(200, 200, 205)))
                        cornerRadius = dp(2).toFloat()
                    }
                },
                LinearLayout.LayoutParams(dp(36), dp(4)).apply {
                    gravity = Gravity.CENTER_HORIZONTAL
                    bottomMargin = dp(10)
                },
            )
            addView(titleRow)
            addView(heightSlider.view)
            addView(raiseSlider.view)
        }
        val sheet = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(controls, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(footprint.root, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        footprint.setSizes(heightDp, raiseDp)

        dialog.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE)
        dialog.setContentView(sheet)
        dialog.setOnDismissListener { onClosed() }
        dialog.window?.let { window ->
            // The stock dialog background carries its own inset and rounded
            // card; clearing it is what lets the sheet span the full width
            // and sit flush with the bottom edge.
            window.setBackgroundDrawable(android.graphics.drawable.ColorDrawable(Color.TRANSPARENT))
            window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            window.setGravity(Gravity.BOTTOM)
            window.setWindowAnimations(android.R.style.Animation_InputMethod)
        }
        dialog.show()
    }

    private class SteppedSlider(val view: View, val set: (Int) -> Unit)

    /**
     * Slider row for the keyboard-size sheet: the value reads as a plain
     * "300 dp" on the right, and −/+ buttons give the single-dp nudges a
     * drag can't hit reliably. `set` moves the slider from code (恢复默认)
     * and reports through onChange exactly like a drag would.
     */
    private fun steppedSliderRow(label: String, min: Int, max: Int, current: Int, onChange: (Int) -> Unit): SteppedSlider {
        var value = current.coerceIn(min, max)
        val textColor = tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44))
        val valueView = TextView(this).apply {
            text = "$value dp"
            textSize = 15f
            setTextColor(textColor)
        }
        val seekBar = SeekBar(this)
        seekBar.max = (max - min).coerceAtLeast(1)
        seekBar.progress = value - min
        fun update(next: Int, fromSeekBar: Boolean) {
            val clamped = next.coerceIn(min, max)
            if (clamped == value) return
            value = clamped
            valueView.text = "$value dp"
            if (!fromSeekBar) seekBar.progress = value - min
            onChange(value)
        }
        seekBar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                if (fromUser) update(progress + min, fromSeekBar = true)
            }
            override fun onStartTrackingTouch(seekBar: SeekBar?) = Unit
            override fun onStopTrackingTouch(seekBar: SeekBar?) = Unit
        })
        fun stepButton(symbol: String, delta: Int): View = TextView(this).apply {
            text = symbol
            textSize = 18f
            gravity = Gravity.CENTER
            setTextColor(textColor)
            background = android.graphics.drawable.GradientDrawable().apply {
                shape = android.graphics.drawable.GradientDrawable.OVAL
                setColor(tone(Color.rgb(60, 60, 60), Color.rgb(225, 225, 228)))
            }
            isClickable = true
            setOnClickListener { update(value + delta, fromSeekBar = false) }
        }
        val header = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        header.addView(
            TextView(this).apply {
                text = label
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        header.addView(valueView)
        val controls = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(6), 0, 0)
        }
        controls.addView(stepButton("−", -1), LinearLayout.LayoutParams(dp(34), dp(34)))
        controls.addView(seekBar, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        controls.addView(stepButton("+", 1), LinearLayout.LayoutParams(dp(34), dp(34)))
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
            addView(header)
            addView(controls)
        }
        return SteppedSlider(row, set = { update(it, fromSeekBar = false) })
    }

    /**
     * Full-width 1:1 IME footprint at the bottom of the keyboard-size sheet.
     * Key-area block uses the stretch height; raise strip is empty lift
     * space below keys.
     */
    private fun buildKeyboardFootprintPreview(): KeyboardFootprintPreview {
        val caption = TextView(this).apply {
            textSize = 11f
            gravity = Gravity.CENTER
            setTextColor(tone(Color.rgb(160, 160, 160), Color.rgb(100, 100, 105)))
            setPadding(0, dp(6), 0, dp(4))
        }
        val keysBlock = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            setPadding(dp(12), dp(10), dp(12), dp(10))
            // Three fake key rows so stretch is visually obvious.
            repeat(3) { rowIndex ->
                val row = LinearLayout(this@OpenLessKeyboardSettingsActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER
                }
                repeat(10) {
                    row.addView(
                        View(this@OpenLessKeyboardSettingsActivity).apply {
                            setBackgroundColor(tone(Color.rgb(70, 70, 70), Color.rgb(255, 255, 255)))
                        },
                        LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f).apply {
                            marginStart = dp(2)
                            marginEnd = dp(2)
                        },
                    )
                }
                addView(
                    row,
                    LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                        if (rowIndex > 0) topMargin = dp(6)
                    },
                )
            }
        }
        val raiseBlock = FrameLayout(this).apply {
            setBackgroundColor(tone(Color.rgb(36, 36, 36), Color.rgb(220, 220, 224)))
            addView(
                TextView(this@OpenLessKeyboardSettingsActivity).apply {
                    text = ui("抬高", "Raise")
                    textSize = 11f
                    gravity = Gravity.CENTER
                    setTextColor(tone(Color.rgb(140, 140, 140), Color.rgb(120, 120, 125)))
                },
                FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(tone(Color.rgb(22, 22, 22), Color.rgb(230, 230, 234)))
            addView(caption, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(keysBlock, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(OpenLessImeService.DEFAULT_KEYBOARD_HEIGHT_DP)))
            addView(raiseBlock, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0))
        }
        return KeyboardFootprintPreview(root, caption, keysBlock, raiseBlock)
    }

    private inner class KeyboardFootprintPreview(
        val root: LinearLayout,
        private val caption: TextView,
        private val keysBlock: View,
        private val raiseBlock: View,
    ) {
        fun setSizes(heightDp: Int, raiseDp: Int) {
            keysBlock.layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                dp(heightDp),
            )
            raiseBlock.layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                dp(raiseDp),
            )
            raiseBlock.visibility = if (raiseDp > 0) View.VISIBLE else View.GONE
            caption.text = ui(
                "1:1 预览 · 键盘共占屏幕底部 ${heightDp + raiseDp}dp",
                "1:1 preview · keyboard takes the bottom ${heightDp + raiseDp}dp",
            )
            root.requestLayout()
        }
    }

    /** One labeled slider row. Reusable as more settings rows get added here. */
    private fun sliderRow(label: String, min: Int, max: Int, current: Int, onChange: (Int) -> Unit, onRelease: (() -> Unit)? = null): View {
        // Computed before building the SeekBar itself, since inside that
        // view's own apply{} block an unqualified "max" would resolve to
        // SeekBar's own max property (shadowing this function's max: Int
        // parameter), not the value intended here.
        val range = (max - min).coerceAtLeast(1)
        val initialProgress = (current - min).coerceIn(0, range)
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
        }
        val labelView = TextView(this).apply {
            text = "$label · Max $max Set:$current"
            textSize = 14f
            setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
        }
        row.addView(labelView)
        row.addView(
            SeekBar(this).apply {
                this.max = range
                this.progress = initialProgress
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                        if (fromUser) {
                            val value = progress + min
                            labelView.text = "$label · Max $max Set:$value"
                            onChange(value)
                        }
                    }
                    override fun onStartTrackingTouch(seekBar: SeekBar?) = Unit
                    override fun onStopTrackingTouch(seekBar: SeekBar?) {
                        onRelease?.invoke()
                    }
                })
            },
        )
        return row
    }

    /** One labeled single-line text field, auto-saving on every keystroke (matches every other row on this page — no separate save button). Used by the "云笔记提交" section for its URL/token; reusable for any future free-text setting. */
    private fun textFieldRow(label: String, hint: String, initial: String, isSecret: Boolean = false, onChange: (String) -> Unit): View {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
        }
        row.addView(
            TextView(this).apply {
                text = label
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                setPadding(0, 0, 0, dp(4))
            },
        )
        row.addView(
            EditText(this).apply {
                setText(initial)
                this.hint = hint
                textSize = 14f
                isSingleLine = true
                if (isSecret) {
                    inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
                }
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
                setHintTextColor(tone(Color.rgb(110, 110, 115), Color.rgb(170, 170, 175)))
                addTextChangedListener(object : android.text.TextWatcher {
                    override fun afterTextChanged(s: android.text.Editable?) = onChange(s?.toString().orEmpty())
                    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
                })
            },
        )
        return row
    }
}
