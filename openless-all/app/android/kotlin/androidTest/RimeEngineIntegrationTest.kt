package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
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
}
