package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyboardUiKitTest {
    @Test
    fun lightAndDarkThemesUseDistinctSemanticSurfaces() {
        val light = KeyboardUiKit.tokens(false)
        val dark = KeyboardUiKit.tokens(true)

        assertNotEquals(light.canvas, dark.canvas)
        assertNotEquals(light.sheet, dark.sheet)
        assertNotEquals(light.primaryKey, dark.primaryKey)
        assertNotEquals(light.accent, dark.accent)
        assertNotEquals(0xFF303030.toInt(), light.canvas)
        assertNotEquals(0xFFF2F2F6.toInt(), light.sheet)
    }

    @Test
    fun semanticGeometryHasTouchSafeRoundedHierarchy() {
        val tokens = KeyboardUiKit.tokens(false)

        assertTrue(tokens.sheetRadiusDp > tokens.cardRadiusDp)
        assertTrue(tokens.cardRadiusDp >= tokens.keyRadiusDp)
        assertTrue(tokens.minTouchTargetDp >= 48)
        assertTrue(tokens.candidateHeightDp >= tokens.minTouchTargetDp)
        assertTrue(tokens.statusCardHeightDp > tokens.headerHeightDp)
        assertTrue(tokens.pressedScale in 0.9f..1f)
    }

    @Test
    fun widthClassesKeepModeToggleAndKeysBounded() {
        assertEquals(KeyboardWidthClass.COMPACT, KeyboardUiKit.widthClass(280))
        assertEquals(KeyboardWidthClass.REGULAR, KeyboardUiKit.widthClass(360))
        assertEquals(KeyboardWidthClass.WIDE, KeyboardUiKit.widthClass(480))

        listOf(280, 360, 480).forEach { width ->
            val toggle = KeyboardUiKit.modeToggleWidthDp(width)
            val keyHeight = KeyboardUiKit.keyHeightDp(width)
            assertTrue(toggle in 178..288)
            assertTrue(keyHeight in 42..50)
        }
    }
}
