package com.lelloman.talia

import android.app.Application
import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import org.json.JSONObject

@RunWith(AndroidJUnit4::class)
class NativeConnectionTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val application = instrumentation.targetContext.applicationContext as Application
    private val vault get() = SessionVault(application)
    private val token = "n." + "a".repeat(43)
    @Before fun prepare() { vault.clear() }
    @After fun cleanup() { vault.clear() }
    private fun seed() { vault.write(JSONObject().put("server", "https://talia.test").put("token", token)) }
    private fun waitForIdle(model: NativeConnection) {
        val deadline = System.currentTimeMillis() + 5000
        do {
            var idle = false
            instrumentation.runOnMainSync { idle = !model.busy }
            if (idle) return
            Thread.sleep(20)
        } while (System.currentTimeMillis() < deadline)
        fail("Connection did not finish")
    }
    @Test fun encryptedStorageAndHttpsBoundary() {
        seed()
        assertEquals(token, vault.read()!!.getString("token"))
        val stored = application.getSharedPreferences("native_session", Context.MODE_PRIVATE).getString("sealed", "")!!
        assertFalse(stored.contains(token))
        assertEquals("https://talia.test", NativeApi.origin("https://talia.test/"))
        for (invalid in listOf("http://talia.test", "https://user:pass@talia.test", "https://talia.test/path", "https://talia.test#fragment")) {
            assertTrue(runCatching { NativeApi.origin(invalid) }.isFailure)
        }
    }
    @Test fun expiryClearsIdentityCachedDataAndStoredCredentials() {
        seed()
        lateinit var model: NativeConnection
        var expire = false
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, path, _, _ ->
                if (expire) throw ApiFailure(401)
                if (path.endsWith("session")) JSONObject().put("name", "Admin") else JSONObject().put("services", org.json.JSONArray())
            }
            model.refresh()
        }
        waitForIdle(model)
        assertNotNull(model.overview)
        assertEquals("Admin", model.name)
        instrumentation.runOnMainSync { expire = true; model.refresh() }
        waitForIdle(model)
        assertFalse(model.signedIn)
        assertNull(model.overview)
        assertNull(vault.read())
        assertTrue(model.message!!.contains("expired"))
    }
    @Test fun outageRetainsLastResponseButRevokedPermissionClearsIt() {
        seed()
        lateinit var model: NativeConnection
        var status = 200
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, path, _, _ ->
                if (path.endsWith("session")) JSONObject().put("name", "Admin")
                else if (status != 200) throw ApiFailure(status)
                else JSONObject().put("fetchedAt", 123)
            }
            model.refresh()
        }
        waitForIdle(model)
        instrumentation.runOnMainSync { status = 503; model.refresh() }
        waitForIdle(model)
        assertEquals(123, model.overview!!.getInt("fetchedAt"))
        assertTrue(model.signedIn)
        instrumentation.runOnMainSync { status = 403; model.refresh() }
        waitForIdle(model)
        assertNull(model.overview)
        assertTrue(model.forbidden)
    }
    @Test fun expiredGatewayCallbackIsReportedWithoutCrashingOrOpeningBrowser() {
        SessionVault(application, "gateway_session").clear()
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, _, _, _ -> error("No native request expected") }
            model.completeGateway(HomelabGateway.CALLBACK) { error("No browser expected") }
        }
        waitForIdle(model)
        assertTrue(model.message!!.contains("expired"))
        assertFalse(model.gateway.pending)
        model.gateway.close()
    }
    @Test fun gatewayDenialDoesNotExpireTheIndependentNativeSession() {
        seed()
        lateinit var model: NativeConnection
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, _, _, _ ->
                throw RemoteAccessFailure("Remote access was revoked")
            }
            model.refresh()
        }
        waitForIdle(model)
        assertTrue(model.signedIn)
        assertEquals(token, vault.read()!!.getString("token"))
        assertEquals("Remote access was revoked", model.message)
    }
    @Test fun nativeHandoffPersistsAndRedeemsPkceWithoutCallbackCredentials() {
        lateinit var model: NativeConnection
        var browserUrl = ""
        var challenge = ""
        instrumentation.runOnMainSync {
            model = NativeConnection(application) { _, path, _, body ->
                when (path) {
                    "/native/start" -> { challenge = body!!.getString("challenge"); JSONObject().put("attempt", "b".repeat(43)) }
                    "/native/exchange" -> {
                        val proof = android.util.Base64.encodeToString(java.security.MessageDigest.getInstance("SHA-256").digest(body!!.getString("verifier").toByteArray()), android.util.Base64.URL_SAFE or android.util.Base64.NO_WRAP or android.util.Base64.NO_PADDING)
                        assertEquals(challenge, proof)
                        JSONObject().put("token", token)
                    }
                    "/native/session" -> JSONObject().put("name", "Admin")
                    else -> JSONObject().put("services", org.json.JSONArray())
                }
            }
            model.signIn("https://talia.test") { browserUrl = it }
        }
        waitForIdle(model)
        assertTrue(model.pending)
        assertTrue(browserUrl.startsWith("https://talia.test/native/authorize?attempt="))
        assertFalse(browserUrl.contains(vault.read()!!.getString("verifier")))
        instrumentation.runOnMainSync { model.refresh() }
        waitForIdle(model)
        assertTrue(model.signedIn)
        assertFalse(model.pending)
        assertEquals(token, vault.read()!!.getString("token"))
        assertFalse(vault.read()!!.has("verifier"))
    }
}
