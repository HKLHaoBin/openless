package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors

/**
 * Full-pinyin single-character and phrase lookup plus abbreviation lookup for
 * Pinyin mode. All dictionary access and learning writes stay on one worker;
 * callers receive immutable candidate metadata on the main looper.
 */
internal class LitePinyinRepository(context: Context) : PinyinInputEngine {
    private val appContext = context.applicationContext
    private val userFrequency = LitePinyinUserFrequency(context)
    private val learnedPhrases = LitePinyinLearnedPhrases(context)
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-pinyin-candidate").apply { isDaemon = true }
    }
    private val mainHandler = Handler(Looper.getMainLooper())

    private data class Entry(
        val text: String,
        val weight: Int,
        val abbreviation: String? = null,
    )
    private data class CandidateEntry(
        val sourceKey: String,
        val entry: Entry,
        val source: PinyinCandidateSource,
        val learningKey: String? = null,
    )

    private val charIndex = HashMap<String, List<Entry>>()
    private val phraseFullPinyinIndex = HashMap<String, List<Entry>>()
    private val abbreviationIndex = HashMap<String, List<Entry>>()
    private val syllableSet = HashSet<String>()

    @Volatile
    private var loaded = false

    override fun query(
        encoding: String,
        personalize: Boolean,
        callback: (List<PinyinCandidate>) -> Unit,
    ) {
        val normalized = normalizeKey(encoding)
        if (normalized.isEmpty()) {
            callback(emptyList())
            return
        }
        executor.execute {
            ensureLoaded()
            val entries = candidateEntries(normalized)
            fun rankEntries(candidates: List<CandidateEntry>): List<PinyinCandidate> {
                val previouslySelected = candidates
                    .filter { personalize && userFrequency.score(it.sourceKey, it.entry.text) > 0.0 }
                    .sortedByDescending { userFrequency.score(it.sourceKey, it.entry.text) }
                val rest = candidates.filterNot { entry ->
                    previouslySelected.any {
                        it.entry.text == entry.entry.text && it.sourceKey == entry.sourceKey
                    }
                }
                return (previouslySelected + rest).map { candidate ->
                    PinyinCandidate(
                        text = candidate.entry.text,
                        sourceKey = candidate.sourceKey,
                        consumedLength = candidate.sourceKey.length,
                        source = candidate.source,
                        learningKey = candidate.learningKey ?: learningKey(candidate.sourceKey, candidate.source),
                    )
                }
            }

            var ranked = (
                rankEntries(entries.filter { it.sourceKey == normalized }) +
                    rankEntries(entries.filter { it.sourceKey != normalized })
                ).distinctBy { Triple(it.text, it.sourceKey, it.source) }
            if (personalize) {
                learnedPhrases.promoted(normalized)?.let { learned ->
                    ranked = listOf(
                        PinyinCandidate(
                            text = learned,
                            sourceKey = normalized,
                            consumedLength = normalized.length,
                            source = PinyinCandidateSource.LEARNED,
                            learningKey = normalized,
                        ),
                    ) + ranked.filterNot { it.text == learned }
                }
            }
            mainHandler.post { callback(ranked.take(20)) }
        }
    }

    override fun recordSelection(candidate: PinyinCandidate) {
        val sourceKey = normalizeKey(candidate.sourceKey)
        if (sourceKey.isEmpty() || candidate.text.isEmpty()) return
        executor.execute { userFrequency.record(sourceKey, candidate.text) }
    }

    override fun observeSequence(combinedEncoding: String, text: String) {
        val normalized = normalizeKey(combinedEncoding)
        if (normalized.isEmpty() || text.isEmpty()) return
        executor.execute { learnedPhrases.observeSequence(normalized, text) }
    }

    override fun preloadAsync() {
        executor.execute { ensureLoaded() }
    }

    override fun shutdown() {
        executor.shutdownNow()
    }

    private fun candidateEntries(normalized: String): List<CandidateEntry> {
        val exact = exactEntries(normalized, PinyinCandidateSource.FULL_PINYIN)
        val firstSyllable = segmentEncoding(normalized).firstOrNull()
        if (firstSyllable == null || firstSyllable.length >= normalized.length) return exact
        val fallback = exactEntries(firstSyllable, PinyinCandidateSource.SYLLABLE)
        return (exact + fallback).distinctBy { Triple(it.sourceKey, it.entry.text, it.source) }
    }

    private fun exactEntries(
        key: String,
        fallbackSource: PinyinCandidateSource,
    ): List<CandidateEntry> {
        val chars = charIndex[key].orEmpty().map {
            CandidateEntry(key, it, if (fallbackSource == PinyinCandidateSource.SYLLABLE) fallbackSource else PinyinCandidateSource.FULL_PINYIN)
        }
        val fullPinyinPhrases = phraseFullPinyinIndex[key].orEmpty().map {
            CandidateEntry(
                key,
                it,
                if (fallbackSource == PinyinCandidateSource.SYLLABLE) fallbackSource else PinyinCandidateSource.FULL_PINYIN,
                it.abbreviation,
            )
        }
        val abbreviations = if (key.length >= 2) {
            abbreviationIndex[key].orEmpty().map { CandidateEntry(key, it, PinyinCandidateSource.ABBREVIATION, key) }
        } else {
            emptyList()
        }
        return chars + fullPinyinPhrases + abbreviations
    }

    private fun learningKey(key: String, source: PinyinCandidateSource): String {
        if (source == PinyinCandidateSource.ABBREVIATION) return key
        return segmentEncoding(key)
            .joinToString(separator = "") { it.first().toString() }
            .ifEmpty { key.take(1) }
    }

    private fun segmentEncoding(encoding: String): List<String> {
        val normalized = normalizeKey(encoding)
        if (normalized.isEmpty() || syllableSet.isEmpty()) return emptyList()
        val segments = mutableListOf<String>()
        var offset = 0
        while (offset < normalized.length) {
            val maxLength = minOf(MAX_SYLLABLE_LENGTH, normalized.length - offset)
            var match: String? = null
            for (length in maxLength downTo 1) {
                val candidate = normalized.substring(offset, offset + length)
                if (syllableSet.contains(candidate)) {
                    match = candidate
                    break
                }
            }
            if (match == null) break
            segments += match
            offset += match.length
        }
        return segments
    }

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            loadChars()
            loadPhrases()
            loaded = true
        }
    }

    private fun loadChars() {
        val grouped = HashMap<String, MutableList<Entry>>()
        runCatching {
            appContext.assets.open("pinyin_chars.tsv").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val parts = line.split('\t')
                    if (parts.size < 3) return@forEach
                    val char = parts[0]
                    val pinyin = normalizeKey(parts[1])
                    val weight = parts[2].toIntOrNull() ?: 0
                    if (char.isEmpty() || pinyin.isEmpty()) return@forEach
                    grouped.getOrPut(pinyin) { mutableListOf() }.add(Entry(char, weight))
                }
            }
        }
        grouped.forEach { (pinyin, entries) ->
            charIndex[pinyin] = entries.sortedByDescending { it.weight }
        }
        syllableSet.clear()
        syllableSet.addAll(charIndex.keys)
    }

    private fun loadPhrases() {
        val fullPinyinGrouped = HashMap<String, MutableList<Entry>>()
        val abbreviationGrouped = HashMap<String, MutableList<Entry>>()
        runCatching {
            appContext.assets.open("pinyin_phrases.tsv").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val parts = line.split('\t')
                    if (parts.size < 4) return@forEach
                    val phrase = parts[0]
                    val fullPinyin = normalizeKey(parts[1])
                    val abbreviation = normalizeKey(parts[2])
                    val weight = parts[3].toIntOrNull() ?: 0
                    if (phrase.isEmpty()) return@forEach
                    val entry = Entry(phrase, weight, abbreviation.takeIf { it.isNotEmpty() })
                    if (fullPinyin.isNotEmpty()) fullPinyinGrouped.getOrPut(fullPinyin) { mutableListOf() }.add(entry)
                    if (abbreviation.isNotEmpty()) abbreviationGrouped.getOrPut(abbreviation) { mutableListOf() }.add(entry)
                }
            }
        }
        fullPinyinGrouped.forEach { (key, entries) ->
            phraseFullPinyinIndex[key] = entries.sortedByDescending { it.weight }
        }
        abbreviationGrouped.forEach { (key, entries) ->
            abbreviationIndex[key] = entries.sortedWith(
                compareByDescending<Entry> { it.text in PRIORITY_PHRASES }.thenByDescending { it.weight },
            )
        }
    }

    private companion object {
        const val MAX_SYLLABLE_LENGTH = 6
        val PRIORITY_PHRASES = setOf("输入法", "候选词", "剪贴板", "供应商", "物料", "主管")

        fun normalizeKey(value: String): String = value
            .trim()
            .lowercase()
            .replace("ü", "v")
            .replace("u:", "v")
            .filterNot(Char::isWhitespace)
    }
}
