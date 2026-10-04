package com.lelloman.talia

import android.app.Application
import android.graphics.Bitmap
import android.view.accessibility.AccessibilityNodeInfo
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.ui.Modifier
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.lelloman.lellodesign.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.io.IOException
import kotlinx.coroutines.launch

/** Chat against an in-memory stand-in for `/native/chats` (androidTest only). */
@RunWith(AndroidJUnit4::class)
class ChatsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val application = instrumentation.targetContext.applicationContext as Application
    private val vault get() = SessionVault(application)

    private class FakeServer {
        val sessions = mutableListOf<JSONObject>()
        val requests = mutableListOf<JSONObject>()
        val calls = mutableListOf<JSONObject>()
        var loseNext = false
        var forbid = false
        private var next = 1L
        private fun admit(session: String, args: JSONObject): Long {
            requests.firstOrNull { it.getString("session") == session && it.getString("requestId") == args.getString("requestId") }?.let { return it.getLong("id") }
            val r = JSONObject().put("id", next++).put("session", session).put("requestId", args.getString("requestId")).put("text", args.getString("text"))
                .put("status", "running").put("steps", JSONArray("""[{"label":"Checked the monitoring overview","done":true},{"label":"Read host-homelab history","done":false}]"""))
            requests += r
            sessions.first { it.getString("id") == session }.put("updated", System.currentTimeMillis())
            return r.getLong("id")
        }
        fun finish(answer: String) = requests.filter { it.getString("status") == "running" }.forEach { it.put("status", "done").put("answer", answer).put("steps", JSONArray()) }
        fun handle(body: JSONObject): JSONObject = synchronized(this) {
            calls += JSONObject(body.toString())
            if (forbid) throw ApiFailure(403)
            val a = body.getJSONObject("args")
            val result = when (body.getString("op")) {
                "list" -> JSONObject().put("sessions", JSONArray(sessions.filter { !it.optBoolean("deleted") }.sortedByDescending { it.getLong("updated") }.map { s ->
                    JSONObject(s.toString()).put("running", requests.any { it.getString("session") == s.getString("id") && it.getString("status") == "running" }) }))
                "create" -> {
                    val s = sessions.firstOrNull { it.getString("client") == a.getString("requestId") } ?: JSONObject().put("id", "chat-${next++}")
                        .put("client", a.getString("requestId")).put("title", a.getString("text").lines().first()).put("updated", System.currentTimeMillis()).also { sessions += it }
                    JSONObject().put("session", s.getString("id")).put("request", admit(s.getString("id"), a))
                }
                "send" -> JSONObject().put("session", a.getString("session")).put("request", admit(a.getString("session"), a))
                "get" -> {
                    val id = a.getString("session")
                    if (sessions.none { it.getString("id") == id && !it.optBoolean("deleted") }) throw ApiFailure(404)
                    JSONObject().put("requests", JSONArray(requests.filter { it.getString("session") == id }.map { JSONObject(it.toString()) }))
                }
                "stop" -> { requests.filter { it.getString("session") == a.getString("session") && it.getString("status") == "running" }.forEach { it.put("status", "stopped") }; JSONObject().put("stopped", 1) }
                "rename" -> { sessions.first { it.getString("id") == a.getString("session") }.put("title", a.getString("title")); JSONObject() }
                "delete" -> { sessions.first { it.getString("id") == a.getString("session") }.put("deleted", true); JSONObject().put("deleted", true) }
                else -> throw ApiFailure(400)
            }
            if (loseNext && body.getString("op") in setOf("create", "send")) { loseNext = false; throw IOException("response lost") }
            result
        }
    }

    @Before fun seed() {
        vault.write(JSONObject().put("server", "https://talia.test").put("token", "n." + "a".repeat(43)))
        application.getSharedPreferences("chats", 0).edit().clear().commit()
    }
    @After fun cleanup() { vault.clear(); application.getSharedPreferences("chats", 0).edit().clear().commit() }

    private fun model(server: FakeServer): NativeConnection {
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync { model = NativeConnection(application) { _, path, _, body -> assertEquals("/native/chats", path); server.handle(body!!) } }
        return model
    }
    private fun until(what: String, condition: () -> Boolean) {
        val deadline = System.currentTimeMillis() + 5000
        while (System.currentTimeMillis() < deadline) {
            var ok = false
            instrumentation.runOnMainSync { ok = condition() }
            if (ok) return
            Thread.sleep(20)
        }
        fail("Timed out waiting for $what")
    }
    private fun main(block: () -> Unit) = instrumentation.runOnMainSync(block)

    @Test fun lostResponseRetriesWithoutDuplicatesAndSessionsAreIndependent() {
        val server = FakeServer()
        val model = model(server)
        val chats = model.chats
        main { chats.openSession(""); chats.edit("How is the disk?") ; server.loseNext = true; chats.send() }
        until("lost send") { !chats.busy }
        assertEquals("How is the disk?", chats.draft)
        assertNotNull(chats.unsent)
        val requestId = chats.unsent!!.getString("requestId")
        main { chats.send() }
        until("retry") { !chats.busy && chats.requests.size == 1 }
        val creates = server.calls.filter { it.getString("op") == "create" }
        assertEquals(2, creates.size)
        assertEquals(requestId, creates[1].getJSONObject("args").getString("requestId"))
        assertEquals(1, server.requests.size)
        assertNull(chats.unsent); assertEquals("", chats.draft)
        assertTrue(chats.running)
        val first = chats.open!!
        // A second chat runs independently; stopping it does not touch the first.
        main { chats.openSession(""); chats.edit("Any alerts?"); chats.send() }
        until("second chat") { !chats.busy && chats.open != first && chats.requests.size == 1 }
        main { chats.stop() }
        until("stop") { !chats.busy && chats.requests.firstOrNull()?.getString("status") == "stopped" }
        assertEquals("running", server.requests.first { it.getString("session") == first }.getString("status"))
        main { chats.rename("Alert check") }
        until("rename") { !chats.busy && chats.title == "Alert check" }
        main { chats.delete() }
        until("delete") { !chats.busy && chats.open == null && chats.sessions.size == 1 }
        // A draft survives leaving and reopening a chat.
        main { chats.openSession(first); chats.edit("unfinished thought"); chats.close(); chats.openSession(first) }
        assertEquals("unfinished thought", chats.draft)
        server.forbid = true
        main { kotlinx.coroutines.MainScope().launch { chats.refreshList() } }
        until("forbidden") { chats.forbidden }
        // Signing out discards chat state and stored drafts.
        main { model.cancelSignIn() }
        assertTrue(application.getSharedPreferences("chats", 0).all.isEmpty())
    }

    @Test fun reviewConversationLightAndDark() {
        val server = FakeServer()
        val model = model(server)
        val chats = model.chats
        main { chats.openSession(""); chats.edit("How is the disk on homelab?"); chats.send() }
        until("sent") { !chats.busy && chats.requests.isNotEmpty() }
        main { server.finish("The root filesystem is 41% free; /mnt/data is at 6% and needs attention."); chats.edit("And /mnt/data?"); chats.send() }
        until("second") { !chats.busy && chats.requests.size == 2 }
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            for (dark in listOf(false, true)) {
                scenario.onActivity { activity ->
                    activity.setContent {
                        androidx.compose.runtime.key(dark) {
                            LelloTheme(product = "blue", dark = dark) {
                                LelloScaffold(productName = "Talìa", title = chats.title ?: "Chats", destinations = emptyList(), selectedId = "chats", onNavigate = {}) { insets ->
                                    ChatsScreen(model, Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets)) {}
                                }
                            }
                        }
                    }
                }
                awaitText("Read host-homelab history")
                shot(if (dark) "chat-dark" else "chat-light")
            }
            // Stop through accessibility, as TalkBack would.
            val stop = find(instrumentation.uiAutomation.rootInActiveWindow) { it.text?.toString() == "Stop" }
            var target = stop
            while (target != null && !target.isClickable) target = target.parent
            assertNotNull(target); assertTrue(target!!.performAction(AccessibilityNodeInfo.ACTION_CLICK))
            awaitText("Stopped before an answer was ready.")
            shot("chat-stopped")
        }
    }

    private fun texts(node: AccessibilityNodeInfo?): List<String> {
        if (node == null) return emptyList()
        return listOfNotNull(node.text?.toString(), node.contentDescription?.toString()) + (0 until node.childCount).flatMap { texts(node.getChild(it)) }
    }
    private fun find(node: AccessibilityNodeInfo?, predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (node == null) return null
        if (predicate(node)) return node
        return (0 until node.childCount).firstNotNullOfOrNull { find(node.getChild(it), predicate) }
    }
    private fun awaitText(text: String) {
        val deadline = System.currentTimeMillis() + 10000
        while (texts(instrumentation.uiAutomation.rootInActiveWindow).none { it.contains(text) } && System.currentTimeMillis() < deadline) Thread.sleep(100)
        assertTrue("missing '$text'", texts(instrumentation.uiAutomation.rootInActiveWindow).any { it.contains(text) })
    }
    private fun shot(name: String) {
        val directory = File(instrumentation.targetContext.externalCacheDir, "ui-review").apply { mkdirs() }
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
}

