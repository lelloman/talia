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
import org.json.JSONObject
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class ReportsReviewTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun texts(node: AccessibilityNodeInfo?): List<String> = if (node == null) emptyList() else
        listOfNotNull(node.text?.toString()) + (0 until node.childCount).flatMap { texts(node.getChild(it)) }
    private fun awaitText(expected: String) {
        val deadline = System.currentTimeMillis() + 5000
        while (expected !in texts(instrumentation.uiAutomation.rootInActiveWindow) && System.currentTimeMillis() < deadline) Thread.sleep(50)
        assertTrue("Missing $expected", expected in texts(instrumentation.uiAutomation.rootInActiveWindow))
    }
    private fun clickText(text: String) {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.text?.toString() == text) return node
            for (i in 0 until node.childCount) find(node.getChild(i))?.let { return it }
            return null
        }
        var node = find(instrumentation.uiAutomation.rootInActiveWindow)
        while (node != null) {
            if (node.isClickable && node.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
                instrumentation.waitForIdleSync()
                Thread.sleep(150)
                return
            }
            node = node.parent
        }
        fail("No clickable $text")
    }
    private fun refreshBySwipe(requests: java.util.concurrent.atomic.AtomicInteger) {
        val before = requests.get()
        val display = instrumentation.targetContext.resources.displayMetrics
        val x = display.widthPixels * 0.9f
        val start = display.heightPixels * 0.32f
        val end = display.heightPixels * 0.8f
        val down = android.os.SystemClock.uptimeMillis()
        fun event(action: Int, y: Float) {
            val e = android.view.MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), action, x, y, 0)
            e.source = android.view.InputDevice.SOURCE_TOUCHSCREEN
            instrumentation.uiAutomation.injectInputEvent(e, true)
            e.recycle()
        }
        event(android.view.MotionEvent.ACTION_DOWN, start)
        for (i in 1..25) { Thread.sleep(10); event(android.view.MotionEvent.ACTION_MOVE, start + (end - start) * i / 25) }
        event(android.view.MotionEvent.ACTION_UP, end)
        val deadline = System.currentTimeMillis() + 5000
        while (requests.get() == before && System.currentTimeMillis() < deadline) Thread.sleep(25)
        assertTrue("Swipe should refresh the current Reports screen", requests.get() > before)
        instrumentation.waitForIdleSync()
        Thread.sleep(400)
    }
    private fun shot(name: String) {
        val directory = File(instrumentation.targetContext.externalCacheDir, "ui-review").apply { mkdirs() }
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    @Test fun catalogHistoryAndCompletedResult() {
        val app = instrumentation.targetContext.applicationContext as Application
        val vault = SessionVault(app)
        vault.clear()
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var model: NativeConnection
            val requests = java.util.concurrent.atomic.AtomicInteger()
            scenario.onActivity { activity ->
                vault.write(JSONObject().put("server", "https://talia.test").put("token", "n." + "a".repeat(43)))
                model = NativeConnection(app) { _, _, _, body ->
                    requests.incrementAndGet()
                    when (body!!.getString("op")) {
                        "list" -> {
                            val rows = org.json.JSONArray()
                            listOf("homelab-infrastructure", "service-health", "storage-capacity", "daily-summary", "network-checks", "weekly-review").forEachIndexed { i, name ->
                                val row = JSONObject().put("id", name).put("enabled", true).put("scheduled", true).put("steps", 8).put("period_ms", 86400000).put("available", true)
                                if (i < 5) row.put("latest", JSONObject().put("status", listOf("partial", "complete", "failed", "running", "complete")[i]).put("created", 1790578800000L - i * 3600000))
                                rows.put(row)
                            }
                            JSONObject().put("definitions", rows)
                        }
                        "runs" -> JSONObject("""{"runs":[{"id":"fixture-run","status":"complete","created":1790578800000}],"next_before":null}""")
                        else -> JSONObject("""{"run":{"id":"fixture-run","report":"homelab-infrastructure","status":"complete","period_start":1790492400000,"created":1790578800000,"send":false,"steps":[{"id":"service-health","status":"complete","error":null}],"error":null,"deliveries":[],"content":{"subject":"Infra report: Warning","summary":"One service needs attention.","sections":[{"title":"Service checks","text":"Pezzottify is reachable. Simple Agents needs attention."}]}}}""")
                    }
                }
                activity.setContent {
                    LelloTheme(product = "blue") {
                        LelloScaffold(productName = "Talìa", title = "Reports", destinations = emptyList(), selectedId = "reports", onNavigate = {}) { insets ->
                            ReportsScreen(model, Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets)) {}
                        }
                    }
                }
            }
            awaitText("6 of 6 reports")
            assertFalse("Available reports" in texts(instrumentation.uiAutomation.rootInActiveWindow))
            assertFalse("Refresh" in texts(instrumentation.uiAutomation.rootInActiveWindow))
            refreshBySwipe(requests)
            shot("reports-catalog")
            clickText("Filter"); awaitText("Never run"); shot("reports-filter-sheet"); clickText("Never run")
            clickText("Apply"); awaitText("1 of 6 reports"); shot("reports-applied-filter")
            clickText("Status: Never run")
            awaitText("6 of 6 reports")
            instrumentation.runOnMainSync { model.selectReport("homelab-infrastructure") }
            awaitText("Completed"); refreshBySwipe(requests); shot("reports-history")
            instrumentation.runOnMainSync { model.selectReportRun("fixture-run") }
            awaitText("Infra report: Warning"); refreshBySwipe(requests); shot("reports-result")
            assertTrue("Run completed" in texts(instrumentation.uiAutomation.rootInActiveWindow))
        }
        vault.clear()
    }
}
