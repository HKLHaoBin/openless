package com.openless.app

import android.content.ContextWrapper
import android.text.Editable
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ImeAutoTextIntegrationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test fun observeCandidateSpaceAfterCaretMove() = instrumentation.runOnMainSync {
        val (service, edit) = editor("old new ")
        setField(service, "englishAutoSpacePending", true)
        edit.setSelection(4)
        service.onUpdateSelection(8, 8, 4, 4, -1, -1)
        call(service, "commitEnglishChar", ",")
        val changedUnrelatedSpace = edit.text.toString() == "old, new "
        probe("P1", "H1", changedUnrelatedSpace)
        assertTrue(changedUnrelatedSpace)
    }

    @Test fun observeDoubleSpaceAfterCaretMove() = instrumentation.runOnMainSync {
        val (service, edit) = editor("one two ")
        edit.setSelection(3)
        call(service, "commitEnglishSpace")
        edit.setSelection(edit.length())
        service.onUpdateSelection(4, 4, 9, 9, -1, -1)
        call(service, "commitEnglishSpace")
        val changedUnrelatedSpace = edit.text.toString().endsWith("two. ")
        probe("P2", "H2", changedUnrelatedSpace)
        assertTrue(changedUnrelatedSpace)
    }

    @Test fun observeSplitAssociationLearning() {
        val lookup: (String) -> List<StrokePhraseRepository.Candidate> = { prefix ->
            listOf("天气", "天气预报").filter { it.startsWith(prefix) }
                .map { StrokePhraseRepository.Candidate(it, 1, prefix) }
        }
        assertEquals(null, typedContinuation("天气", "预", lookup))
        val completed = typedContinuation("天气预", "报", lookup)
        val usesLaterPrefix = completed?.matchedPrefix == "天气预"
        probe("P3", "H3", usesLaterPrefix)
        assertTrue(usesLaterPrefix)
    }

    private fun editor(text: String): Pair<OpenLessImeService, EditText> {
        val service = OpenLessImeService()
        ContextWrapper::class.java.getDeclaredMethod("attachBaseContext", android.content.Context::class.java)
            .apply { isAccessible = true }.invoke(service, instrumentation.targetContext)
        service.inputMode = OpenLessImeService.InputMode.ENGLISH
        val edit = EditText(instrumentation.targetContext).apply {
            setText(text)
            setSelection(length())
        }
        val connection = object : BaseInputConnection(edit, true) {
            override fun getEditable(): Editable = edit.text
            override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText = ExtractedText().apply {
                this.text = edit.text.toString()
                startOffset = 0
                selectionStart = edit.selectionStart
                selectionEnd = edit.selectionEnd
            }
        }
        setField(service, "draftEdit", edit)
        setField(service, "draftConnection", connection)
        return service to edit
    }

    private fun setField(service: OpenLessImeService, name: String, value: Any) {
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.set(service, value)
    }

    private fun call(service: OpenLessImeService, name: String, vararg args: String) {
        OpenLessImeService::class.java.getDeclaredMethod(name, *args.map { String::class.java }.toTypedArray())
            .apply { isAccessible = true }.invoke(service, *args)
    }

    private fun probe(id: String, hypothesis: String, observed: Boolean) {
        val record = JSONObject().apply {
            put("schema", "diagnosing-bugs.v1")
            put("session_id", "a5809584")
            put("run_id", "ime1230-baseline")
            put("probe_id", id)
            put("hypothesis_ids", org.json.JSONArray(listOf(hypothesis)))
            put("event", "regression_boundary")
            put("outcome", if (observed) "bug_observed" else "bug_absent")
            put("safe", JSONObject().put("matched", observed))
        }
        synchronized(ImeAutoTextIntegrationTest::class.java) {
            java.io.File(instrumentation.targetContext.filesDir, "ime1230-a5809584.ndjson")
                .appendText("$record\n")
        }
    }
}
