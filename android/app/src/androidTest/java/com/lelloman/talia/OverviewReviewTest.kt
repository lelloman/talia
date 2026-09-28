package com.lelloman.talia

import android.graphics.Bitmap
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.lelloman.lellodesign.*
import org.json.JSONObject
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import android.view.accessibility.AccessibilityNodeInfo
import java.io.File

/** Fixture screenshots stay in androidTest; no fake monitoring data ships in the app. */
@RunWith(AndroidJUnit4::class)
class OverviewReviewTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun fixture(stale: Boolean): JSONObject = JSONObject("""{
      "fetchedAt":${System.currentTimeMillis()}, "sampledAt":${System.currentTimeMillis()}, "stale":$stale,
      "services":[{"name":"Pezzottify","status":"up"},{"name":"Simple Agents","status":"down"},{"name":"Home Assistant","status":"up"}],
      "reports":[{"id":"fixture-run-20260928","report":"infra","title":"Infrastructure report","status":"succeeded","created":${System.currentTimeMillis()},"summary":"One service needs attention. Simple Agents is unreachable; Pezzottify and Home Assistant are reachable."}]
    }""")
    private fun texts(node: AccessibilityNodeInfo?): List<String> {
        if (node == null) return emptyList()
        return listOfNotNull(node.text?.toString()) + (0 until node.childCount).flatMap { texts(node.getChild(it)) }
    }
    private fun scroll(node: AccessibilityNodeInfo?): Boolean {
        if (node == null) return false
        if (node.isScrollable && node.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)) return true
        return (0 until node.childCount).any { scroll(node.getChild(it)) }
    }
    private fun shot(name: String) {
        val directory = File(instrumentation.targetContext.externalCacheDir, "ui-review").apply { mkdirs() }
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    @Test fun reviewLightDarkAndStaleLargeText() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            for ((name, dark, stale) in listOf(Triple("overview-light", false, false), Triple("overview-dark", true, false), Triple("overview-stale-large", false, true))) {
                scenario.onActivity { activity ->
                    androidx.core.view.WindowCompat.getInsetsController(activity.window, activity.window.decorView).apply {
                        isAppearanceLightStatusBars = !dark
                        isAppearanceLightNavigationBars = !dark
                    }
                    activity.setContent {
                      androidx.compose.runtime.key(name) {
                        val density = LocalDensity.current
                        CompositionLocalProvider(LocalDensity provides Density(density.density, if (stale) 1.5f else 1f)) {
                            LelloTheme(product = "blue", dark = dark) {
                                LelloScaffold(productName = "Talìa", title = "Overview", destinations = emptyList(), selectedId = "overview", onNavigate = {}) { insets ->
                                    LelloWorkspace(Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets).verticalScroll(rememberScrollState())) {
                                        OverviewContent(fixture(stale), false, null, false, "Secure tunnel", {})
                                    }
                                }
                            }
                        }
                      }
                    }
                }
                instrumentation.waitForIdleSync()
                // Await rendered semantics, not just the Activity/main thread becoming idle.
                val deadline = System.currentTimeMillis() + 5000
                val expected = if (stale) "Status is out of date" else "Services need attention"
                while (expected !in texts(instrumentation.uiAutomation.rootInActiveWindow) && System.currentTimeMillis() < deadline) Thread.sleep(50)
                assertTrue(texts(instrumentation.uiAutomation.rootInActiveWindow).contains(expected))
                shot(name)
                if (!stale) {
                    scroll(instrumentation.uiAutomation.rootInActiveWindow)
                    instrumentation.waitForIdleSync()
                    Thread.sleep(400)
                    shot("$name-reports")
                }
            }
        }
    }
}
