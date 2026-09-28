package com.lelloman.talia

import android.app.Application
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.IOException

@RunWith(AndroidJUnit4::class)
class ReportsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val application = instrumentation.targetContext.applicationContext as Application
    private val vault get() = SessionVault(application)
    @Before fun seed() { vault.write(JSONObject().put("server", "https://talia.test").put("token", "n." + "a".repeat(43))) }
    @After fun cleanup() { vault.clear() }
    private fun idle(model: NativeConnection) {
        val deadline = System.currentTimeMillis() + 5000
        do {
            var busy = true
            instrumentation.runOnMainSync { busy = model.reportsBusy }
            if (!busy) return
            Thread.sleep(20)
        } while (System.currentTimeMillis() < deadline)
        fail("Reports request did not finish")
    }
    @Test fun catalogSearchFiltersAndSortingCompose() {
        val data = org.json.JSONArray("""[
          {"id":"alpha","enabled":true,"scheduled":true,"available":true,"latest":{"status":"complete","created":100}},
          {"id":"beta","enabled":false,"scheduled":false,"available":true,"latest":{"status":"failed","created":300}},
          {"id":"gamma","enabled":true,"scheduled":true,"available":true,"latest":{"status":"running","created":200}},
          {"id":"delta","enabled":false,"scheduled":false,"available":true,"latest":null}]
        """)
        fun ids(query: String = "", sort: String = "newest", filter: String = "all") = catalogReports(data, query, sort, filter).map { it.getString("id") }
        assertEquals(listOf("beta", "gamma", "alpha", "delta"), ids())
        assertEquals(listOf("alpha", "gamma", "beta", "delta"), ids(sort = "oldest"))
        assertEquals(listOf("alpha", "beta", "delta", "gamma"), ids(sort = "name"))
        assertEquals(listOf("beta"), ids(query = " BE ", filter = "issues"))
        assertEquals(listOf("gamma"), ids(filter = "active"))
        assertEquals(listOf("gamma", "alpha"), ids(filter = "scheduled"))
        assertEquals(listOf("delta"), ids(filter = "never"))
        assertEquals(emptyList<String>(), ids(query = "missing"))
    }
    @Test fun changingHistoryFiltersResetsCursorAndAppliesToEveryPage() {
        val queries = mutableListOf<JSONObject>()
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, _, _, body ->
                queries.add(JSONObject(body!!.getJSONObject("args").toString()))
                JSONObject("""{"runs":[{"id":"r1"}],"next_before":"r1"}""")
            }
            model.selectReport("infra")
        }
        idle(model)
        instrumentation.runOnMainSync { model.moreReportRuns() }
        idle(model)
        instrumentation.runOnMainSync { model.filterReportHistory("oldest", "failed") }
        idle(model)
        assertEquals(1, model.reportHistory.length())
        assertFalse(queries.last().has("before"))
        instrumentation.runOnMainSync { model.moreReportRuns() }
        idle(model)
        assertEquals("r1", queries.last().getString("before"))
        assertEquals("oldest", queries.last().getString("sort"))
        assertEquals("failed", queries.last().getString("status"))
    }
    @Test fun lostAdmissionResponseRetriesSameRequestAfterRestartAndPollsResult() {
        var requestId = ""
        var admissions = 0
        var first = true
        val request: suspend (String, String, String?, JSONObject?) -> JSONObject = { _, path, token, body ->
            assertEquals("/native/reports", path)
            assertTrue(token!!.startsWith("n."))
            val args = body!!.getJSONObject("args")
            when (body.getString("op")) {
                "runs" -> JSONObject("""{"runs":[],"next_before":null}""")
                "run" -> {
                    assertFalse(args.getBoolean("send"))
                    if (first) { first = false; requestId = args.getString("requestId"); admissions++; kotlinx.coroutines.delay(100); throw IOException("lost response") }
                    assertEquals(requestId, args.getString("requestId"))
                    JSONObject("""{"run_id":"r1","status":"queued"}""")
                }
                else -> JSONObject("""{"run":{"id":"r1","status":"complete","content":{"summary":"All checked"}}}""")
            }
        }
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync { model = NativeConnection(application, request); model.selectReport("infra") }
        idle(model)
        instrumentation.runOnMainSync { model.runReport(); model.runReport() }
        idle(model)
        assertEquals("infra", model.pendingReport)
        assertNotNull(model.reportsMessage)
        instrumentation.runOnMainSync { model = NativeConnection(application, request); model.runReport() }
        idle(model)
        assertEquals(1, admissions)
        assertNull(model.pendingReport)
        assertEquals("r1", model.reportRunSelected)
        assertEquals("complete", model.reportDetail!!.getString("status"))
        assertFalse(vault.read()!!.has("reportRequest"))
    }
    @Test fun pagingKeepsExistingRunsAndPermissionRevocationClearsCachedResults() {
        var forbidden = false
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, _, _, body ->
                if (forbidden) throw ApiFailure(403)
                val args = body!!.getJSONObject("args")
                if (args.has("before")) {
                    assertEquals("r2", args.getString("before"))
                    JSONObject("""{"runs":[{"id":"r1"}],"next_before":null}""")
                } else JSONObject("""{"runs":[{"id":"r2"}],"next_before":"r2"}""")
            }
            model.selectReport("infra")
        }
        idle(model)
        instrumentation.runOnMainSync { model.moreReportRuns() }
        idle(model)
        assertEquals(2, model.reportHistory.length())
        assertNull(model.reportsNext)
        instrumentation.runOnMainSync { forbidden = true; model.refreshReports() }
        idle(model)
        assertEquals(0, model.reportHistory.length())
        assertTrue(model.reportsForbidden)
        assertTrue(model.signedIn)
    }
    @Test fun definitiveRejectionAllowsNewRequestAndExpiryClearsEverything() {
        lateinit var model: NativeConnection
        var status = 400
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, _, _, body ->
                if (body!!.getString("op") == "runs") JSONObject("""{"runs":[],"next_before":null}""") else throw ApiFailure(status)
            }
            model.selectReport("infra")
        }
        idle(model)
        instrumentation.runOnMainSync { model.runReport() }
        idle(model)
        assertNull(model.pendingReport)
        assertFalse(vault.read()!!.has("reportRequest"))
        instrumentation.runOnMainSync { status = 401; model.runReport() }
        idle(model)
        assertFalse(model.signedIn)
        assertNull(vault.read())
        assertEquals("", model.reportSelected)
    }
}
