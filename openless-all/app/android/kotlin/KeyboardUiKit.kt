package com.openless.app

/** Semantic visual tokens shared by every native keyboard panel. */
internal data class KeyboardUiTokens(
    val canvas: Int,
    val sheet: Int,
    val card: Int,
    val primaryKey: Int,
    val secondaryKey: Int,
    val candidate: Int,
    val voice: Int,
    val accent: Int,
    val destructive: Int,
    val primaryText: Int,
    val secondaryText: Int,
    val divider: Int,
    val keyRadiusDp: Int,
    val cardRadiusDp: Int,
    val sheetRadiusDp: Int,
    val keyGapDp: Int,
    val minTouchTargetDp: Int,
    val headerHeightDp: Int,
    val candidateHeightDp: Int,
    val statusCardHeightDp: Int,
    val keyTextSp: Float,
    val secondaryTextSp: Float,
    val statusTextSp: Float,
    val pressedScale: Float,
)

internal enum class KeyboardSurfaceRole {
    CANVAS,
    SHEET,
    CARD,
    KEY,
    SECONDARY_KEY,
    CANDIDATE,
    VOICE,
    DESTRUCTIVE,
}

internal enum class KeyboardWidthClass {
    COMPACT,
    REGULAR,
    WIDE,
}

internal object KeyboardUiKit {
    private val LIGHT_CANVAS = 0xFFE9EFF1.toInt()
    private val LIGHT_SHEET = 0xFFF8FBFC.toInt()
    private val LIGHT_CARD = 0xFFFFFFFF.toInt()
    private val LIGHT_PRIMARY_KEY = 0xFFFFFFFF.toInt()
    private val LIGHT_SECONDARY_KEY = 0xFFDDE8EC.toInt()
    private val LIGHT_CANDIDATE = 0xFFE6F2F2.toInt()
    private val LIGHT_VOICE = 0xFFD3F2F0.toInt()
    private val LIGHT_ACCENT = 0xFF007C83.toInt()
    private val LIGHT_DESTRUCTIVE = 0xFFB3263E.toInt()
    private val LIGHT_PRIMARY_TEXT = 0xFF13232B.toInt()
    private val LIGHT_SECONDARY_TEXT = 0xFF60727B.toInt()
    private val LIGHT_DIVIDER = 0xFFC9D6DA.toInt()

    private val DARK_CANVAS = 0xFF0F171B.toInt()
    private val DARK_SHEET = 0xFF182229.toInt()
    private val DARK_CARD = 0xFF25343D.toInt()
    private val DARK_PRIMARY_KEY = 0xFF2B3A44.toInt()
    private val DARK_SECONDARY_KEY = 0xFF344651.toInt()
    private val DARK_CANDIDATE = 0xFF24454A.toInt()
    private val DARK_VOICE = 0xFF1C4B50.toInt()
    private val DARK_ACCENT = 0xFF75E0D5.toInt()
    private val DARK_DESTRUCTIVE = 0xFFFF8798.toInt()
    private val DARK_PRIMARY_TEXT = 0xFFF4F8FA.toInt()
    private val DARK_SECONDARY_TEXT = 0xFFAABAC2.toInt()
    private val DARK_DIVIDER = 0xFF3B4B54.toInt()

    fun tokens(darkTheme: Boolean): KeyboardUiTokens = if (darkTheme) {
        KeyboardUiTokens(
            canvas = DARK_CANVAS,
            sheet = DARK_SHEET,
            card = DARK_CARD,
            primaryKey = DARK_PRIMARY_KEY,
            secondaryKey = DARK_SECONDARY_KEY,
            candidate = DARK_CANDIDATE,
            voice = DARK_VOICE,
            accent = DARK_ACCENT,
            destructive = DARK_DESTRUCTIVE,
            primaryText = DARK_PRIMARY_TEXT,
            secondaryText = DARK_SECONDARY_TEXT,
            divider = DARK_DIVIDER,
            keyRadiusDp = 14,
            cardRadiusDp = 16,
            sheetRadiusDp = 22,
            keyGapDp = 6,
            minTouchTargetDp = 48,
            headerHeightDp = 44,
            candidateHeightDp = 48,
            statusCardHeightDp = 72,
            keyTextSp = 20f,
            secondaryTextSp = 12f,
            statusTextSp = 15f,
            pressedScale = 0.965f,
        )
    } else {
        KeyboardUiTokens(
            canvas = LIGHT_CANVAS,
            sheet = LIGHT_SHEET,
            card = LIGHT_CARD,
            primaryKey = LIGHT_PRIMARY_KEY,
            secondaryKey = LIGHT_SECONDARY_KEY,
            candidate = LIGHT_CANDIDATE,
            voice = LIGHT_VOICE,
            accent = LIGHT_ACCENT,
            destructive = LIGHT_DESTRUCTIVE,
            primaryText = LIGHT_PRIMARY_TEXT,
            secondaryText = LIGHT_SECONDARY_TEXT,
            divider = LIGHT_DIVIDER,
            keyRadiusDp = 14,
            cardRadiusDp = 16,
            sheetRadiusDp = 22,
            keyGapDp = 6,
            minTouchTargetDp = 48,
            headerHeightDp = 44,
            candidateHeightDp = 48,
            statusCardHeightDp = 72,
            keyTextSp = 20f,
            secondaryTextSp = 12f,
            statusTextSp = 15f,
            pressedScale = 0.965f,
        )
    }

    fun widthClass(widthDp: Int): KeyboardWidthClass = when {
        widthDp < 320 -> KeyboardWidthClass.COMPACT
        widthDp >= 420 -> KeyboardWidthClass.WIDE
        else -> KeyboardWidthClass.REGULAR
    }

    fun modeToggleWidthDp(widthDp: Int): Int = when (widthClass(widthDp)) {
        KeyboardWidthClass.COMPACT -> (widthDp * 0.58f).toInt().coerceIn(180, 208)
        KeyboardWidthClass.REGULAR -> (widthDp * 0.62f).toInt().coerceIn(204, 252)
        KeyboardWidthClass.WIDE -> (widthDp * 0.48f).toInt().coerceIn(230, 288)
    }

    fun keyHeightDp(widthDp: Int): Int = when (widthClass(widthDp)) {
        KeyboardWidthClass.COMPACT -> 42
        KeyboardWidthClass.REGULAR -> 46
        KeyboardWidthClass.WIDE -> 50
    }
}
