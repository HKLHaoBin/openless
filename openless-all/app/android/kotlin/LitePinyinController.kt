package com.openless.app

import android.content.Context

/**
 * Owns the Pinyin-mode encoding buffer AND its candidate query pipeline on
 * the English keyboard panel (see OpenLessImeService.LatinInputMode) —
 * phases 3+4 of the lite-pinyin plan (docs/pinyin-lite/phase-0-audit.md):
 * single-character full-pinyin lookup merged with high-frequency
 * abbreviation-phrase lookup (LitePinyinRepository.query() does the actual
 * merging/ranking; this class just owns the buffer and epoch around it).
 *
 * Deliberately does not touch currentInputConnection or any View — callers
 * get results through the [onCandidates] callback and decide what to do
 * with them (render a row, commit a character, etc.) — see the lite-pinyin
 * plan's 5.2 ("不让 View 直接查询词库或持有 Repository").
 */
internal enum class PinyinCandidateSource {
    LEARNED,
    FULL_PINYIN,
    ABBREVIATION,
    SYLLABLE,
}

internal data class PinyinCandidate(
    val text: String,
    val sourceKey: String,
    val consumedLength: Int,
    val source: PinyinCandidateSource,
    val queryEpoch: Long = 0L,
    val learningKey: String = "",
)

internal interface PinyinInputEngine {
    fun query(
        encoding: String,
        personalize: Boolean,
        callback: (List<PinyinCandidate>) -> Unit,
    )

    fun recordSelection(candidate: PinyinCandidate)
    fun observeSequence(combinedEncoding: String, text: String)
    fun preloadAsync()
    fun shutdown()
}

internal class LitePinyinController internal constructor(
    private val engine: PinyinInputEngine,
) {
    internal constructor(context: Context) : this(LitePinyinRepository(context))
    private val encoding = StringBuilder()
    private var learningAllowed = false
    fun setLearningAllowed(allowed: Boolean) {
        learningAllowed = allowed
        clear()
        resetAssociationContext()
    }
    fun shutdown() = engine.shutdown()

    // --- Two-step combo learning (见 LitePinyinLearnedPhrases 的文档注释) ---
    // "上次直接打拼音上屏的是什么" — separate from confirmedText above
    // (which accumulates without bound until a panel/session boundary):
    // this only ever remembers the ONE immediately-preceding commit, and is
    // cleared at the same boundaries so a chain never spans a mode switch.
    private var lastCommitEncoding: String? = null
    private var lastCommitText: String? = null

    // Independent from englishCandidateQueryEpoch/strokeQueryEpoch/
    // phraseQueryEpoch by design (plan 8.3: "不得复用笔画 epoch 变量，避免
    // 不同输入模式互相影响") — bumped on every state change so a slow
    // background query for an encoding the user has since edited or
    // cleared can never overwrite what's on screen now.
    private var queryEpoch = 0L

    fun currentEncoding(): String = encoding.toString()

    fun isEmpty(): Boolean = encoding.isEmpty()

    fun preloadAsync() = engine.preloadAsync()

    /**
     * Record frequency under the candidate's real sourceKey, then drop only
     * that source encoding from the buffer (full clear when the pick matched
     * the whole buffer; first-syllable consume when it matched the leading
     * syllable — including multi-character phrases keyed on that syllable).
     */
    fun isCandidateCurrent(candidate: PinyinCandidate): Boolean {
        val currentEncoding = encoding.toString()
        return candidate.queryEpoch == queryEpoch &&
            candidate.consumedLength > 0 &&
            candidate.consumedLength <= currentEncoding.length &&
            currentEncoding.startsWith(candidate.sourceKey)
    }

    fun commitSelection(candidate: PinyinCandidate, onCandidates: (List<PinyinCandidate>) -> Unit): Boolean {
        if (!isCandidateCurrent(candidate)) return false
        if (learningAllowed) engine.recordSelection(candidate)
        if (candidate.consumedLength >= encoding.length) {
            clear()
            onCandidates(emptyList())
            return true
        }
        encoding.delete(0, candidate.consumedLength)
        query(onCandidates)
        return true
    }

    /**
     * Call with the encoding still intact — i.e. before clear() — for every
     * DIRECT pinyin/abbreviation commit (not an association pick — see
     * OpenLessImeService.selectPinyinAssociation(), which doesn't call this
     * in this first version). If this immediately follows another such
     * commit (nothing else broke the chain — see resetAssociationContext()
     * for what counts as "broke"), records the two-step combo with
     * LitePinyinLearnedPhrases so a real habit (not a one-off) eventually
     * surfaces on its own — see that class's own doc comment.
     */
    fun observeCommitForLearning(candidate: PinyinCandidate) {
        if (!learningAllowed) return
        val text = candidate.text
        val currentEncoding = encoding.toString()
        // A single full-pinyin syllable ("guo") contributes only its first
        // letter, matching how a real abbreviation is built one letter per
        // character. Repository candidates provide the exact key for phrases;
        // the fallback keeps injected engines compatible with the old rule.
        val abbreviation = candidate.learningKey.ifEmpty {
            if (currentEncoding.length == text.length) currentEncoding else currentEncoding.take(1)
        }
        val previousAbbreviation = lastCommitEncoding
        val previousText = lastCommitText
        if (previousAbbreviation != null && previousText != null) {
            engine.observeSequence(previousAbbreviation + abbreviation, previousText + text)
        }
        lastCommitEncoding = abbreviation
        lastCommitText = text
    }

    /** Letters only — OpenLessImeService is responsible for routing non-letter keys elsewhere. */
    fun appendLetter(char: Char, onCandidates: (List<PinyinCandidate>) -> Unit) {
        if (!char.isLetter()) return
        encoding.append(char.lowercaseChar())
        associationEpoch++
        query(onCandidates)
    }

    /** @return true if a character was actually removed (false when the encoding was already empty, meaning the caller should fall back to normal backspace). */
    fun backspace(onCandidates: (List<PinyinCandidate>) -> Unit): Boolean {
        if (encoding.isEmpty()) return false
        encoding.deleteCharAt(encoding.length - 1)
        associationEpoch++
        query(onCandidates)
        return true
    }

    /** Mode switch, panel switch, new input session, or a space press — never carries a half-typed encoding across any of these (plan 3.2 point 8/9). Deliberately does NOT touch the association context below — that's meant to survive individual commits/mode toggles, only reset at true panel-switch/session boundaries (see resetAssociationContext()). */
    fun clear() {
        encoding.clear()
        queryEpoch++
        associationEpoch++
    }

    // --- Post-commit association (取最后几个字联想下一个词，与笔画面板共用同一份联想词库+调频) ---
    // Deliberately NOT the same buffer/epoch as the encoding above: this one
    // survives every individual candidate commit (that's the entire point —
    // it's what lets one word's commit suggest the next), only reset at
    // panel-switch/new-session boundaries, mirroring
    // StrokeInputController's own confirmedText exactly (same
    // MAX_ASSOCIATION_CONTEXT cap, cleared at the same two call sites:
    // selectInputMode()'s mode-switch reset and onStartInput()'s session
    // reset — see resetAssociationContext()).
    private val confirmedText = StringBuilder()
    private var associationEpoch = 0L

    /** Feeds just-committed on-screen text into the association context — call after every pinyin commit (a fresh candidate or an association suffix alike). */
    fun recordCommittedText(text: String) {
        if (!learningAllowed) return
        confirmedText.append(text)
        val overflow = confirmedText.length - StrokeInputController.MAX_ASSOCIATION_CONTEXT
        if (overflow > 0) confirmedText.delete(0, overflow)
    }

    /**
     * Association candidates for whatever's currently in the shared
     * context. [phraseRepository] is passed in rather than owned here — the
     * caller hands over StrokeInputController's own instance (see that
     * class's phraseRepository doc comment for why this is the SAME
     * ~220k-entry index Stroke mode uses, not a second copy), so usage
     * recorded from either input mode benefits both.
     */
    fun queryAssociations(phraseRepository: StrokePhraseRepository, onResults: (List<StrokePhraseRepository.Candidate>) -> Unit) {
        val context = confirmedText.toString()
        val epoch = ++associationEpoch
        if (context.isEmpty()) {
            onResults(emptyList())
            return
        }
        phraseRepository.searchAsync(context) { result ->
            if (epoch == associationEpoch) onResults(result)
        }
    }

    /** Panel switch or new input session only — an individual commit must NOT call this (see this section's own doc comment above). Also breaks the two-step combo-learning chain (lastCommitEncoding/Text) for the same reason. */
    fun resetAssociationContext() {
        confirmedText.clear()
        associationEpoch++
        lastCommitEncoding = null
        lastCommitText = null
    }

    private fun query(onCandidates: (List<PinyinCandidate>) -> Unit) {
        if (encoding.isEmpty()) {
            queryEpoch++
            onCandidates(emptyList())
            return
        }
        val epoch = ++queryEpoch
        engine.query(encoding.toString(), personalize = learningAllowed) { results ->
            val tagged = results.map { it.copy(queryEpoch = epoch) }
            if (epoch == queryEpoch) onCandidates(tagged)
        }
    }
}
