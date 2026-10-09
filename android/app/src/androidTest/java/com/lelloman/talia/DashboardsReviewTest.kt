package com.lelloman.talia

import android.graphics.Bitmap
import android.view.accessibility.AccessibilityNodeInfo
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.lelloman.lellodesign.*
import com.lelloman.talia.dashboard.compose.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/** Drives the real dashboard session and Compose renderer against a fixture server (androidTest only). */
@RunWith(AndroidJUnit4::class)
class DashboardsReviewTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val fixture = JSONObject(instrumentation.context.assets.open("dashboard-fixture.json").bufferedReader().use { it.readText() })
        .also { fixture ->
            // Delivered dashboards use these chart properties. An older packaged
            // validator rejected them before the dashboard could render at all.
            fun chartLabels(node: JSONObject) {
                if (node.optString("type") == "Chart") {
                    node.getJSONObject("props").put("primaryLabel", "Average").put("secondaryLabel", "Max")
                }
                val children = node.optJSONArray("children")
                for (i in 0 until (children?.length() ?: 0)) chartLabels(children!!.getJSONObject(i))
            }
            val definitions = fixture.getJSONObject("package").getJSONObject("definitions")
            definitions.keys().forEach { chartLabels(definitions.getJSONObject(it)) }
        }
    private val engineOps = mutableListOf<String>()
    private val runs = mutableListOf<String>()

    private val transport = object : DashboardTransport {
        override suspend fun dashboards(body: JSONObject) = JSONObject().put("value", fixture.getJSONObject("catalog"))
        override suspend fun clients(body: JSONObject, installation: String): JSONObject {
            assertTrue(installation.length == 64)
            val value = when (body.getString("op")) {
                "register" -> JSONObject().put("clientId", "c1").put("name", "Talìa Android")
                "openSlot", "selectionState" -> JSONObject().put("revision", 1)
                "delivery" -> JSONObject().put("assignment", JSONObject().put("dashboardId", "homelab").put("revision", 1)
                    .put("params", JSONObject()).put("presentation", JSONObject())).put("package", fixture.getJSONObject("package"))
                else -> JSONObject()
            }
            return JSONObject().put("value", value)
        }
        override suspend fun engine(body: JSONObject): JSONObject {
            val op = body.getString("op"); synchronized(engineOps) { engineOps += op }
            // Every engine request carries the delivered dashboard context, like the web client.
            assertEquals("homelab", body.getJSONObject("dashboard").getString("id"))
            val snapshot = JSONObject().put("revision", 1).put("values", fixture.getJSONArray("values")).put("monitoringError", JSONObject.NULL)
            val value: Any = when (op) {
                "run" -> { runs += body.getJSONObject("args").getString("id"); JSONObject().put("status", "complete").put("actionId", body.getJSONObject("args").getString("actionId")) }
                "read" -> fixture.getJSONArray("values").getJSONObject(0)
                else -> snapshot
            }
            return JSONObject().put("version", 1).put("epoch", body.getLong("epoch")).put("incarnation", "fixture").put("value", value)
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
    private fun await(text: String) {
        val deadline = System.currentTimeMillis() + 15000
        while (texts(instrumentation.uiAutomation.rootInActiveWindow).none { it.contains(text) } && System.currentTimeMillis() < deadline) Thread.sleep(100)
        val visible = texts(instrumentation.uiAutomation.rootInActiveWindow)
        assertTrue("missing '$text'; visible: $visible", visible.any { it.contains(text) })
    }
    private fun shot(name: String) {
        val directory = File(instrumentation.targetContext.externalCacheDir, "ui-review").apply { mkdirs() }
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }

    @Test fun rendersHomelabAndRoundTripsActions() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            for (dark in listOf(false, true)) {
                lateinit var session: DashboardSession
                scenario.onActivity { activity ->
                    session = DashboardSession(activity, transport)
                    activity.setContent {
                        androidx.compose.runtime.key(dark) {
                            LelloTheme(product = "blue", dark = dark) {
                                LelloScaffold(productName = "Talìa", title = "homelab", destinations = listOf(
                                    LelloDestination("overview", "Overview") { Icon(DashboardIcon, null) },
                                    LelloDestination("dashboards", "Dashboards") { Icon(DashboardIcon, null) }),
                                    selectedId = "dashboards", onNavigate = {}, mobileNavigation = LelloMobileNavigation.DrawerAndBottom) { insets ->
                                    BoxWithConstraints(Modifier.fillMaxSize().padding(insets)) {
                                        LaunchedEffect(Unit) { session.resize((maxWidth - 32.dp).value); session.open("homelab") }
                                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
                                            (session.state as? DashboardState.Ready)?.tree?.let { DashboardContent(it, session::dispatch, Modifier.fillMaxWidth()) }
                                            (session.state as? DashboardState.Failed)?.let { LelloAlert(it.message, tone = LelloTone.Error) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                await("Host metrics reachable")
                await("Past 24 hours · 5-minute CPU averages")
                shot(if (dark) "dashboard-dark" else "dashboard-light")
                if (!dark) {
                    // The segmented control dispatches to the ViewModel, which re-presents the chart.
                    val hour = find(instrumentation.uiAutomation.rootInActiveWindow) { it.contentDescription?.toString() == "Show past hour of CPU" }
                    var target = hour
                    while (target != null && !target.isClickable) target = target.parent
                    assertNotNull(target); assertTrue(target!!.isCheckable)
                    assertTrue(target.performAction(AccessibilityNodeInfo.ACTION_CLICK))
                    await("Past hour · 1-minute CPU averages")
                    find(instrumentation.uiAutomation.rootInActiveWindow) { it.isScrollable }?.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)
                    Thread.sleep(500)
                    await("41.3% free")
                    // Physical phones can show fewer cards than the emulator.
                    for (attempt in 0 until 6) {
                        if (texts(instrumentation.uiAutomation.rootInActiveWindow).any { it.contains("Git worktrees need attention") }) break
                        find(instrumentation.uiAutomation.rootInActiveWindow) { it.isScrollable }?.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)
                        Thread.sleep(400)
                    }
                    await("Git worktrees need attention")
                    shot("dashboard-storage")
                    find(instrumentation.uiAutomation.rootInActiveWindow) { it.isScrollable }?.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)
                    Thread.sleep(500)
                    shot("dashboard-repositories")
                    val check = find(instrumentation.uiAutomation.rootInActiveWindow) { it.text?.toString() == "Check now" && it.isClickable }
                        ?: find(instrumentation.uiAutomation.rootInActiveWindow) { it.isClickable && texts(it).contains("Check now") }
                    assertNotNull(check); assertTrue(check!!.performAction(AccessibilityNodeInfo.ACTION_CLICK))
                    val deadline = System.currentTimeMillis() + 10000
                    while (runs.isEmpty() && System.currentTimeMillis() < deadline) Thread.sleep(100)
                    assertEquals(listOf("git-recheck-homelab"), runs)
                }
                scenario.onActivity { session.close() }
            }
            assertTrue(synchronized(engineOps) { engineOps.containsAll(listOf("hello", "subscribe", "snapshot")) })
        }
    }
}
