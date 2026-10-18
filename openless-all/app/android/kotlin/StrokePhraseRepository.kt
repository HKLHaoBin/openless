package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.LinkedHashMap
import java.util.concurrent.Executors

/** Independent confirmed-text -> phrase predictor; it never reads stroke codes. */
internal class StrokePhraseRepository(context: Context) {
    data class Candidate(val text: String, val baseWeight: Int, val matchedPrefix: String)

    private class Node {
        val children = HashMap<Char, Node>()
        val top = ArrayList<Candidate>(NODE_TOP_N)
    }

    private val appContext = context.applicationContext
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-phrase-query").apply { isDaemon = true }
    }
    private val root = Node()
    private val cache = object : LinkedHashMap<String, List<Candidate>>(CACHE_SIZE, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<Candidate>>?) = size > CACHE_SIZE
    }
    private var loaded = false

    /**
     * Warm the trie while the IME is idle, same reasoning as
     * StrokeInputRepository.preloadAsync() — this dictionary is ~220k
     * phrases (vs. stroke.dict.tsv's much smaller character list), and
     * every insert() does a linear dedupe-check + a re-sort of up to
     * NODE_TOP_N candidates at every one of a phrase's up to 8 node
     * levels, so building it cold has a real, measurable cost (see the
     * "phrase index ready" log below). Left uncalled until now, so this
     * cost used to land on whichever word's commit happened to be the
     * first one to ever need an association in this repository's
     * lifetime, instead of on IME startup where the user isn't yet
     * waiting on a result.
     */
    fun preloadAsync() {
        executor.execute { ensureLoaded() }
    }

    fun searchAsync(prefix: String, callback: (List<Candidate>) -> Unit) {
        if (prefix.isEmpty()) return callback(emptyList())
        executor.execute {
            val startedAt = android.os.SystemClock.elapsedRealtime()
            ensureLoaded()
            val result = rankAssociations(findLongestSuffix(prefix)) {
                userFrequency.score(it.matchedPrefix.ifEmpty { prefix }, it.text)
            }
            // Temporary-diagnostic-grade, not removed after: cheap (one
            // log line per commit) and directly answers "is this slow
            // because of the cold trie build, the query itself, or is it
            // not finding anything at all" without guessing — see
            // openless-all/app/android README for how to read it via
            // logcat while reproducing a report of slow/missing
            // associations.
            android.util.Log.d(
                "OpenLessPhrase",
                "query prefix=\"$prefix\" results=${result.size} elapsed=${android.os.SystemClock.elapsedRealtime() - startedAt}ms",
            )
            Handler(Looper.getMainLooper()).post { callback(result) }
        }
    }

    /** Records that `candidate` was committed after `context`, off the caller's thread. */
    fun recordUsage(context: String, candidate: String) {
        executor.execute { userFrequency.record(context, candidate) }
    }

    /**
     * [word] was typed (not picked from the association row) right after
     * [contextBefore]. If that is exactly one of the associations the row
     * offered for [contextBefore], count it as a pick of that association —
     * under the same (matched prefix, phrase) key a tap would have used, so
     * typing a phrase out by hand trains the row too. Off the caller's thread.
     */
    fun recordTypedContinuation(contextBefore: String, word: String) {
        if (contextBefore.isEmpty() || word.isEmpty()) return
        executor.execute {
            ensureLoaded()
            val hit = typedContinuation(contextBefore, word, ::find) ?: return@execute
            userFrequency.record(hit.matchedPrefix, hit.text)
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            val startedAt = android.os.SystemClock.elapsedRealtime()
            var entryCount = 0
            runCatching {
                appContext.assets.open("phrases.dict.tsv").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val parts = line.split('\t', limit = 2)
                        val phrase = parts.getOrNull(0) ?: return@forEach
                        val weight = parts.getOrNull(1)?.toIntOrNull() ?: 0
                        if (phrase.length in 2..8 && weight > 0) {
                            insert(Candidate(phrase, weight, ""))
                            entryCount++
                        }
                    }
                }
            }
            // Every node touched during insert() only appended (see below);
            // this single pass is where dedupe + sort + trim-to-top-N
            // actually happens now, once per node regardless of how many
            // times that node was touched while loading — insert() used to
            // do a full re-sort of the node's list on every single touch,
            // ~220k times across up to 8 node levels each.
            finalizeNode(root)
            loaded = true
            android.util.Log.i(
                "OpenLessPhrase",
                "phrase index ready entries=$entryCount elapsed=${android.os.SystemClock.elapsedRealtime() - startedAt}ms",
            )
        }
    }

    /** Appends only — no dedupe/sort/trim here, see finalizeNode(). */
    private fun insert(candidate: Candidate) {
        var node = root
        candidate.text.forEach { character ->
            node = node.children.getOrPut(character) { Node() }
            node.top += candidate
        }
    }

    /** One-time post-load pass: dedupe (keep first-seen, matching insert()'s old none{} check), sort, trim to NODE_TOP_N, then recurse. Trie depth is capped at 8 (phrase length), so recursion depth is never a concern. */
    private fun finalizeNode(node: Node) {
        if (node.top.size > 1) {
            val finalized = node.top.distinctBy { it.text }.sortedByDescending { it.baseWeight }.take(NODE_TOP_N)
            node.top.clear()
            node.top.addAll(finalized)
        }
        node.children.values.forEach { finalizeNode(it) }
    }

    /**
     * Memoized per suffix length actually tried, not per full (rolling)
     * caller-supplied prefix — findLongestSuffix() below tries decreasing
     * suffix lengths of `prefix` until one matches, and during continuous
     * typing the full rolling prefix is different on almost every call
     * while its trailing few characters (the substrings actually looked up
     * here) recur constantly, so caching at this level is what actually
     * gets hit. Behavior is unchanged either way — this only memoizes each
     * find() call findLongestSuffix() would have made anyway, in the same
     * longest-first order.
     */
    private fun find(prefix: String): List<Candidate> {
        synchronized(cache) { cache[prefix] }?.let { return it }
        var node = root
        for (character in prefix) {
            node = node.children[character] ?: run {
                val empty = emptyList<Candidate>()
                synchronized(cache) { cache[prefix] = empty }
                return empty
            }
        }
        val result = node.top.map { it.copy(matchedPrefix = prefix) }
        synchronized(cache) { cache[prefix] = result }
        return result
    }

    private fun findLongestSuffix(prefix: String): List<Candidate> = longestSuffixExtensions(prefix, ::find)

    private companion object {
        const val NODE_TOP_N = 12
        // Bumped from 64 now that the cache is keyed by short trailing
        // substrings (bounded cardinality — common 1-3 character endings)
        // instead of the full rolling context, so it holds far more useful
        // distinct entries for the same memory budget.
        const val CACHE_SIZE = 256
    }

    private val userFrequency = StrokeUserFrequency(context)
}

/**
 * Association candidates for [context]: the phrases under its longest
 * trailing substring that [lookup] knows, minus the phrase that substring
 * already completes. Every phrase sits in the trie node of its own last
 * character too, so once a whole phrase is on screen the lookup returns it
 * again — a candidate with nothing left to commit, which tapping can't
 * clear. When that phrase is all there is, the result is empty rather than
 * a retry with a shorter substring: the row should clear, not jump to
 * phrases that only share the last character or two.
 */
internal fun longestSuffixExtensions(
    context: String,
    lookup: (String) -> List<StrokePhraseRepository.Candidate>,
): List<StrokePhraseRepository.Candidate> {
    for (length in context.length downTo 1) {
        val result = lookup(context.takeLast(length))
        if (result.isNotEmpty()) return result.filter { it.text != it.matchedPrefix }
    }
    return emptyList()
}

/**
 * Order for the association row: phrases the user has picked before (a
 * positive [userScore]) first, by that score, then the rest by corpus
 * weight. Corpus weights run from 1 to several hundred thousand while a
 * user score stays in the hundreds, so adding the two — the old order —
 * let a habit move only rare phrases; same reasoning as the stroke
 * candidate order in StrokeInputRepository.searchAsync().
 */
internal fun rankAssociations(
    candidates: List<StrokePhraseRepository.Candidate>,
    userScore: (StrokePhraseRepository.Candidate) -> Double,
): List<StrokePhraseRepository.Candidate> = candidates
    .map { it to userScore(it) }
    .sortedWith(
        compareByDescending<Pair<StrokePhraseRepository.Candidate, Double>> { it.second }
            .thenByDescending { it.first.baseWeight },
    )
    .map { it.first }

/** The association offered for [contextBefore] that [word], typed right after it, completes exactly — or null. */
internal fun typedContinuation(
    contextBefore: String,
    word: String,
    lookup: (String) -> List<StrokePhraseRepository.Candidate>,
): StrokePhraseRepository.Candidate? =
    longestSuffixExtensions(contextBefore, lookup).firstOrNull { it.text == it.matchedPrefix + word }
