package com.lelloman.talia

import android.app.Application
import android.graphics.Bitmap
import android.view.accessibility.AccessibilityNodeInfo
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
            scenario.onActivity { activity ->
                vault.write(JSONObject().put("server", "https://talia.test").put("token", "n." + "a".repeat(43)))
                model = NativeConnection(app) { _, _, _, body ->
                    when (body!!.getString("op")) {
                        "list" -> JSONObject("""{"definitions":[{"id":"homelab-infrastructure","enabled":true,"scheduled":true,"steps":8,"period_ms":86400000,"available":true,"latest":{"status":"partial","created":1790578800000}}]}""")
                        "runs" -> JSONObject("""{"runs":[{"id":"fixture-run","status":"complete","created":1790578800000}],"next_before":null}""")
                        else -> JSONObject("""{"run":{"id":"fixture-run","report":"homelab-infrastructure","status":"complete","period_start":1790492400000,"created":1790578800000,"send":false,"steps":[{"id":"service-health","status":"complete","error":null}],"error":null,"deliveries":[],"content":{"subject":"Infra report: Warning","summary":"One service needs attention.","sections":[{"title":"Service checks","text":"Pezzottify is reachable. Simple Agents needs attention."}]}}}""")
                    }
                }
                activity.setContent {
                    LelloTheme(product = "blue") {
                        LelloScaffold(productName = "Talìa", title = "Reports", destinations = emptyList(), selectedId = "reports", onNavigate = {}) { insets ->
                            LelloWorkspace(Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets).verticalScroll(rememberScrollState())) { Reports(model) {} }
                        }
                    }
                }
            }
            awaitText("Open report"); shot("reports-catalog")
            instrumentation.runOnMainSync { model.selectReport("homelab-infrastructure") }
            awaitText("View run"); shot("reports-history")
            instrumentation.runOnMainSync { model.selectReportRun("fixture-run") }
            awaitText("Infra report: Warning"); shot("reports-result")
            assertTrue("Run completed" in texts(instrumentation.uiAutomation.rootInActiveWindow))
        }
        vault.clear()
    }
}
