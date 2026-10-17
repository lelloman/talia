package com.lelloman.talia

import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.IOException

@RunWith(AndroidJUnit4::class)
class AlertsTest {
    private fun alert() = JSONObject().put("key", "infra-ServiceDown").put("active", true)
        .put("occurrence", 7).put("revision", 12).put("updated_at", 1)
        .put("acknowledgement", JSONObject.NULL)

    @Test fun acknowledgementUsesCurrentOccurrenceAndReloadsServerState() = runBlocking {
        val alert = alert()
        var sent: JSONObject? = null
        val model = AlertModel { body ->
            if (body.getString("op") == "acknowledge") {
                sent = body.getJSONObject("args")
                alert.put("acknowledgement", JSONObject().put("actor", "operator"))
                JSONObject()
            } else JSONObject().put("alerts", JSONArray().put(alert))
        }
        model.refresh()
        model.acknowledge(model.alerts.single())
        assertEquals(7, sent!!.getInt("occurrence"))
        assertEquals(12, sent!!.getInt("expected"))
        assertTrue(sent!!.getString("requestId").isNotEmpty())
        assertNotNull(model.alerts.single().optJSONObject("acknowledgement"))
        assertTrue(model.message!!.startsWith("Acknowledged."))
    }

    @Test fun failedAndStaleAcknowledgementsNeverClaimSuccess() = runBlocking {
        for (failure in listOf(IOException("offline"), ApiFailure(409))) {
            val model = AlertModel { body ->
                if (body.getString("op") == "acknowledge") throw failure
                JSONObject().put("alerts", JSONArray().put(alert()))
            }
            model.refresh()
            model.acknowledge(model.alerts.single())
            assertNull(model.alerts.single().optJSONObject("acknowledgement"))
            assertFalse(model.message!!.startsWith("Acknowledged."))
            assertFalse(model.busy)
        }
    }

    @Test fun viewerDeniedAndOlderServerExplained() = runBlocking {
        for (status in listOf(403, 404)) {
            val model = AlertModel { throw ApiFailure(status) }
            model.refresh()
            assertEquals(status == 403, model.forbidden)
            assertNotNull(model.message)
            assertFalse(model.loaded)
        }
    }
}
