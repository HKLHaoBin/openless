package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** The common Rime engine owns composition; the host owns target-editor writes. */
internal class RimePinyinController(context: Context) {
    private val context = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    @Volatile private var generation = 0L
    @Volatile private var requestId = 0L
    @Volatile private var learning = false
    @Volatile private var encoding = ""
    @Volatile private var candidates: List<String> = emptyList()
    @Volatile private var candidateRevision = 0L
    private var session = ""
    private var pending = ""
    private var commit: (String) -> Boolean = { false }
    private val confirmedText = StringBuilder()
    private var associationEpoch = 0L
    fun bindCommit(handler: (String) -> Boolean) { commit = handler }
    fun currentEncoding(): String = encoding
    fun isEmpty(): Boolean = encoding.isEmpty()
    fun setLearningAllowed(allowed: Boolean) { learning = allowed; clear(); resetAssociationContext() }
    fun preloadAsync() { submit("state", "") {} }
    private fun deliver(text: String, epoch: Long): Boolean {
        val ready = CountDownLatch(1)
        val delivery = AtomicInteger(0) // waiting, executing, complete, cancelled
        var accepted = false
        if (!main.post {
            if (!delivery.compareAndSet(0, 1)) { ready.countDown(); return@post }
            try { if (epoch == generation) accepted = commit(text) }
            finally { delivery.set(2); ready.countDown() }
        }) return false
        if (!ready.await(5, TimeUnit.SECONDS)) {
            if (delivery.compareAndSet(0, 3)) return false
            // A write already executing must finish before allowing a retry.
            ready.await()
        }
        return accepted && epoch == generation
    }
    private fun submit(operation: String, argument: String, update: (List<String>) -> Unit) {
        val epoch = generation
        val revision = ++requestId
        worker.execute {
            try {
                if (epoch != generation) return@execute
                OpenLessRime.initialize(context)
                if (session.isEmpty()) session = OpenLessRime.request("create", "", if (learning) "learning" else "private").getString("session")
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    OpenLessRime.request("ack", session); pending = ""
                }
                var state = OpenLessRime.request(if (operation == "literal") "confirm" else operation, session, argument)
                pending = state.optString("commit")
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    state = OpenLessRime.request("ack", session); pending = ""
                }
                if (operation == "literal" && pending.isEmpty() && argument.isNotEmpty()) {
                    check(deliver(argument, epoch)) { "Editor write failed" }
                }
                val input = state.optString("input")
                val rows = state.optJSONArray("candidates")
                val result = if (rows == null) emptyList() else (0 until rows.length()).map { rows.getString(it) }
                main.post {
                    if (epoch != generation || revision != requestId) return@post
                    encoding = input; candidates = result; candidateRevision = state.optLong("revision"); update(result)
                }
            } catch (error: Throwable) {
                Log.e("OpenLessRime", "engine request failed: ${error.javaClass.simpleName}")
                main.post { if (epoch == generation) update(emptyList()) }
            }
        }
    }
    fun appendLetter(letter: Char, update: (List<String>) -> Unit) {
        encoding += letter.lowercaseChar(); submit("key", letter.lowercaseChar().code.toString(), update)
    }
    fun backspace(update: (List<String>) -> Unit): Boolean {
        if (encoding.isEmpty()) return false
        encoding = encoding.dropLast(1); submit("key", "65288", update); return true
    }
    fun selectCandidate(text: String, update: (List<String>) -> Unit) {
        val index = candidates.indexOf(text)
        if (index >= 0) submit("select", "$index@$candidateRevision", update)
    }
    fun confirmFirst(update: (List<String>) -> Unit): Boolean {
        if (encoding.isEmpty()) return false
        submit("key", "32", update); return true
    }
    fun insertLiteral(text: String, update: (List<String>) -> Unit) { submit("literal", text, update) }
    fun clear() {
        generation++; requestId++; encoding = ""; candidates = emptyList()
        worker.execute {
            try { if (session.isNotEmpty()) OpenLessRime.request("destroy", session) }
            catch (_: Throwable) {} finally { session = ""; pending = "" }
        }
    }
    fun shutdown() { clear(); worker.shutdown() }
    fun recordCommittedText(text: String) {
        if (!learning) return
        confirmedText.append(text)
        if (confirmedText.length > StrokeInputController.MAX_ASSOCIATION_CONTEXT)
            confirmedText.delete(0, confirmedText.length - StrokeInputController.MAX_ASSOCIATION_CONTEXT)
    }
    fun resetAssociationContext() { confirmedText.clear(); associationEpoch++ }
    fun queryAssociations(repository: StrokePhraseRepository, update: (List<StrokePhraseRepository.Candidate>) -> Unit) {
        val text = confirmedText.toString()
        if (text.isEmpty()) { update(emptyList()); return }
        val epoch = ++associationEpoch
        repository.searchAsync(text) { result -> if (epoch == associationEpoch) update(result) }
    }
}
