package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LitePinyinControllerTest {
    @Test
    fun candidateMetadataConsumesOnlyItsConfirmedPrefix() {
        val engine = FakePinyinEngine { encoding ->
            if (encoding == "niha") {
                listOf(PinyinCandidate("你", "ni", 2, PinyinCandidateSource.SYLLABLE))
            } else {
                emptyList()
            }
        }
        val controller = LitePinyinController(engine)
        controller.setLearningAllowed(true)
        var candidates = emptyList<PinyinCandidate>()
        "niha".forEach { controller.appendLetter(it) { candidates = it } }

        val candidate = candidates.single()
        assertEquals(2, candidate.consumedLength)
        assertEquals(PinyinCandidateSource.SYLLABLE, candidate.source)
        assertTrue(controller.commitSelection(candidate) { candidates = it })
        assertEquals("ha", controller.currentEncoding())
        assertEquals(candidate, engine.recordedCandidates.single())
    }

    @Test
    fun staleAsyncResultsAreDiscardedAfterAnotherKeystroke() {
        val engine = DeferredPinyinEngine()
        val controller = LitePinyinController(engine)
        var results = emptyList<PinyinCandidate>()
        controller.appendLetter('n') { results = it }
        controller.appendLetter('i') { results = it }

        engine.respond(
            0,
            listOf(PinyinCandidate("嗯", "n", 1, PinyinCandidateSource.FULL_PINYIN)),
        )
        assertTrue(results.isEmpty())
        engine.respond(
            1,
            listOf(PinyinCandidate("你", "ni", 2, PinyinCandidateSource.FULL_PINYIN)),
        )
        assertEquals(listOf("你"), results.map { it.text })
    }

    @Test
    fun clearRejectsCandidateFromThePreviousEncoding() {
        val engine = FakePinyinEngine {
            listOf(PinyinCandidate("你", it, it.length, PinyinCandidateSource.FULL_PINYIN))
        }
        val controller = LitePinyinController(engine)
        var candidate: PinyinCandidate? = null
        controller.appendLetter('n') { candidate = it.singleOrNull() }
        controller.clear()

        assertFalse(controller.isCandidateCurrent(candidate!!))
    }

    @Test
    fun candidateLearningKeyPreservesPhraseAbbreviation() {
        val engine = FakePinyinEngine { emptyList() }
        val controller = LitePinyinController(engine)
        controller.setLearningAllowed(true)

        controller.observeCommitForLearning(
            PinyinCandidate("你好", "nihao", 5, PinyinCandidateSource.FULL_PINYIN, learningKey = "nh"),
        )
        controller.observeCommitForLearning(
            PinyinCandidate("世界", "shijie", 6, PinyinCandidateSource.FULL_PINYIN, learningKey = "sj"),
        )

        assertEquals(listOf("nhsj" to "你好世界"), engine.observedSequences)
    }

    private class FakePinyinEngine(
        private val candidateFactory: (String) -> List<PinyinCandidate>,
    ) : PinyinInputEngine {
        val recordedCandidates = mutableListOf<PinyinCandidate>()
        val observedSequences = mutableListOf<Pair<String, String>>()

        override fun query(
            encoding: String,
            personalize: Boolean,
            callback: (List<PinyinCandidate>) -> Unit,
        ) = callback(candidateFactory(encoding))

        override fun recordSelection(candidate: PinyinCandidate) {
            recordedCandidates += candidate
        }

        override fun observeSequence(combinedEncoding: String, text: String) {
            observedSequences += combinedEncoding to text
        }
        override fun preloadAsync() = Unit
        override fun shutdown() = Unit
    }

    private class DeferredPinyinEngine : PinyinInputEngine {
        private val callbacks = mutableListOf<(List<PinyinCandidate>) -> Unit>()

        override fun query(
            encoding: String,
            personalize: Boolean,
            callback: (List<PinyinCandidate>) -> Unit,
        ) {
            callbacks += callback
        }

        fun respond(index: Int, candidates: List<PinyinCandidate>) {
            callbacks[index](candidates)
        }

        override fun recordSelection(candidate: PinyinCandidate) = Unit
        override fun observeSequence(combinedEncoding: String, text: String) = Unit
        override fun preloadAsync() = Unit
        override fun shutdown() = Unit
    }
}

class KeyboardLayoutMetricsTest {
    @Test
    fun modeToggleWidthStaysUsableAcrossPhoneWidths() {
        assertEquals(180, KeyboardLayoutMetrics.modeToggleWidthDp(240))
        assertEquals(204, KeyboardLayoutMetrics.modeToggleWidthDp(320))
        assertEquals(230, KeyboardLayoutMetrics.modeToggleWidthDp(480))
    }
}
