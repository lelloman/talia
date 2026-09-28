package com.lelloman.talia

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import it.lelloman.homelab.access.GatewayApi
import it.lelloman.homelab.access.PendingEnrollment
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.net.URI

@RunWith(AndroidJUnit4::class)
class HomelabGatewayTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val gatewayVault get() = SessionVault(context, "gateway_session")
    private val nativeVault get() = SessionVault(context)
    @Before fun prepare() { gatewayVault.clear(); nativeVault.clear() }
    @After fun cleanup() { gatewayVault.clear(); nativeVault.clear() }

    @Test fun gatewayStorageAndCancellationDoNotEraseNativeSession() {
        nativeVault.write(JSONObject().put("token", "native-test-token"))
        gatewayVault.write(JSONObject().put("pending", "opaque-pending").put("expires", Long.MAX_VALUE))
        HomelabGateway(context).use { gateway ->
            assertTrue(gateway.pending)
            gateway.cancel()
            assertFalse(gateway.pending)
            assertEquals("native-test-token", nativeVault.read()!!.getString("token"))
            assertFalse(gatewayVault.read()!!.has("pending"))
        }
    }

    @Test fun encryptedGatewayCredentialSurvivesRecreationIndependentlyOfNativeLogin() {
        val api = GatewayApi(HomelabGateway.GATEWAY)
        val credential = it.lelloman.homelab.access.GatewayCredential("fake-device-secret", "fake-device")
        gatewayVault.write(JSONObject().put("credential", String(api.encodeCredential(credential))))
        val sealed = context.getSharedPreferences("gateway_session", 0).getString("sealed", "")!!
        assertFalse(sealed.contains("fake-device-secret"))
        nativeVault.clear()
        HomelabGateway(context).use { assertTrue(it.enrolled); assertFalse(it.pending) }
    }

    @Test fun callbackMustMatchExactRegisteredAuthorityAndPath() {
        assertTrue(HomelabGateway.isCallback(HomelabGateway.CALLBACK))
        for (url in listOf("https://gateway/callback", "com.lelloman.talia://gateway.evil/callback", "com.lelloman.talia://gateway/other", "com.lelloman.talia://user@gateway/callback", "com.lelloman.talia://gateway:443/callback")) {
            assertFalse(HomelabGateway.isCallback(URI(url)))
        }
    }

    @Test fun forgedAndDuplicateCallbackParametersCannotRedeemEnrollment() = runBlocking {
        val api = GatewayApi(HomelabGateway.GATEWAY)
        val pending = PendingEnrollment("test-enrollment", "expected-state", "verifier", "https://access.lelloman.com/enroll/test-enrollment")
        gatewayVault.write(JSONObject().put("pending", String(api.encodePending(pending))).put("expires", Long.MAX_VALUE))
        HomelabGateway(context).use { gateway ->
            for (query in listOf("state=wrong&enrollment=test-enrollment&code=test", "state=expected-state&state=other&enrollment=test-enrollment&code=test", "state=expected-state&enrollment=other&code=test", "state=expected-state&enrollment=test-enrollment&code=test#fragment")) {
                val result = runCatching { gateway.complete(URI("${HomelabGateway.CALLBACK}?$query")) }
                assertTrue(result.exceptionOrNull() is IllegalArgumentException)
                assertFalse(gateway.enrolled)
                assertTrue(gateway.pending)
            }
        }
    }

    @Test fun expiredEnrollmentIsClearedBeforeAnyNetworkExchange() = runBlocking {
        gatewayVault.write(JSONObject().put("pending", "expired").put("expires", 1))
        HomelabGateway(context).use { gateway ->
            assertTrue(runCatching { gateway.complete(HomelabGateway.CALLBACK) }.exceptionOrNull() is RemoteAccessFailure)
            assertFalse(gateway.pending)
        }
    }
}
