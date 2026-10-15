package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.runner.RunWith

/** Real packaged JNI + dictionary test, using a non-learning disposable session. */
@RunWith(AndroidJUnit4::class)
class RimeEngineIntegrationTest {
    @Test fun candidatesDeletionStaleSelectionAndCommitAcknowledgement() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        var state = OpenLessRime.request("create", "", "private")
        val session = state.getString("session")
        try {
            for (letter in "nihao") state = OpenLessRime.request("key", session, letter.code.toString())
            val oldRevision = state.getLong("revision")
            state = OpenLessRime.request("key", session, "65288")
            assertEquals("niha", state.getString("input"))
            try {
                OpenLessRime.request("select", session, "0@$oldRevision")
                fail("Old candidate must be rejected")
            } catch (_: IllegalStateException) {}
            state = OpenLessRime.request("key", session, "111")
            val candidates = state.getJSONArray("candidates")
            val index = (0 until candidates.length()).first { candidates.getString(it) == "你好" }
            state = OpenLessRime.request("select", session, "$index@${state.getLong("revision")}")
            assertEquals("你好", state.getString("commit"))
            assertEquals("你好", OpenLessRime.request("state", session).getString("commit"))
            assertEquals("", OpenLessRime.request("ack", session).getString("commit"))
            state = OpenLessRime.request("clear", session)
            assertEquals("", state.getString("input"))
            for (letter in "nihao") state = OpenLessRime.request("key", session, letter.code.toString())
            assertEquals("你好", OpenLessRime.request("key", session, "32").getString("commit"))
            OpenLessRime.request("ack", session)
        } finally { OpenLessRime.request("destroy", session) }
        try {
            OpenLessRime.request("state", session)
            fail("Destroyed session must be rejected")
        } catch (_: IllegalStateException) {}
    }
    @Test fun observingAndExpandingCandidatesPreservesSelectionAndNativeOrder() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        val session = OpenLessRime.request("create", "", "private").getString("session")
        try {
            var state = OpenLessRime.request("state", session)
            for (letter in "shi") state = OpenLessRime.request("key", session, letter.code.toString())
            val revision = state.getLong("revision")
            val firstPage = state.getJSONArray("candidates")
            val expanded = OpenLessRime.request("candidates", session)
            assertEquals(revision, expanded.getLong("revision"))
            assertEquals(revision, OpenLessRime.request("state", session).getLong("revision"))
            val rows = expanded.getJSONArray("candidates")
            assertTrue("Candidates beyond the first page must be available", rows.length() > firstPage.length())
            assertTrue(rows.length() <= 64)
            for (i in 0 until firstPage.length()) assertEquals(firstPage.getString(i), rows.getString(i))
            val index = firstPage.length()
            state = OpenLessRime.request("select_absolute", session, "$index@$revision")
            assertEquals(rows.getString(index), state.getString("commit"))
            OpenLessRime.request("ack", session)
        } finally { OpenLessRime.request("destroy", session) }
    }

    private fun withController(test: (RimePinyinController, StringBuilder) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val controller = RimePinyinController(instrumentation.targetContext)
        val committed = StringBuilder()
        instrumentation.runOnMainSync { controller.bindCommit { committed.append(it); true } }
        try { test(controller, committed) }
        finally { instrumentation.runOnMainSync { controller.shutdown() } }
    }

    private fun drain(controller: RimePinyinController, actions: () -> Unit) {
        val done = CountDownLatch(1)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            actions()
            controller.refresh { done.countDown() }
        }
        assertTrue("Input queue did not finish", done.await(30, TimeUnit.SECONDS))
    }

    @Test fun fastSpaceAndBackspaceUseQueuedCompositionRatherThanStaleUi() = withController { controller, committed ->
        val deletions = AtomicInteger()
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.space {}
            // The second space must reach the editor, not the now-empty Rime session.
            controller.space {}
            controller.backspace({}, { deletions.incrementAndGet() })
        }
        assertEquals("你好 ", committed.toString())
        assertEquals(1, deletions.get())
        assertTrue(controller.isEmpty())
        drain(controller) {
            controller.appendLetter('n') {}
            controller.backspace({}, { deletions.incrementAndGet() })
        }
        assertEquals(1, deletions.get())
        assertTrue(controller.isEmpty())
    }

    @Test fun queuedEnterConfirmsOnlyOnceThenUsesEditorAction() = withController { controller, committed ->
        val enters = AtomicInteger()
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.confirmFirst({}, { enters.incrementAndGet() })
            controller.confirmFirst({}, { enters.incrementAndGet() })
        }
        assertEquals("你好", committed.toString())
        assertEquals(1, enters.get())
        assertTrue(controller.isEmpty())
    }

    @Test fun refreshKeepsCompositionAndApostropheSeparatesSyllables() = withController { controller, committed ->
        drain(controller) {
            for (letter in "xi") controller.appendLetter(letter) {}
            controller.insertLiteral("'") {}
            for (letter in "an") controller.appendLetter(letter) {}
        }
        assertEquals("xi'an", controller.currentEncoding())
        assertEquals("", committed.toString())
        drain(controller) { controller.refresh {} }
        assertEquals("xi'an", controller.currentEncoding())
        drain(controller) { controller.selectCandidate("西安") {} }
        assertEquals("西安", committed.toString())
    }

    @Test fun failedCommitRetriesOnceWithoutLosingFollowingPunctuation() = withController { controller, committed ->
        val attempts = AtomicInteger()
        drain(controller) {
            controller.bindCommit { text ->
                if (attempts.incrementAndGet() == 1) false else { committed.append(text); true }
            }
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.insertLiteral("!") {}
        }
        assertEquals("你好!", committed.toString())
        assertEquals(3, attempts.get())
        drain(controller) { controller.refresh {} }
        assertEquals("你好!", committed.toString())
    }

    @Test fun clearingQueuedInputCannotWriteToNextEditor() = withController { controller, committed ->
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.space {}
            controller.clear()
        }
        assertEquals("", committed.toString())
        assertTrue(controller.isEmpty())
    }

}
