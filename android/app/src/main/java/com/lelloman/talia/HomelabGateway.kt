package com.lelloman.talia

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import androidx.compose.runtime.*
import it.lelloman.homelab.access.GatewayApi
import it.lelloman.homelab.access.GatewayCredential
import it.lelloman.homelab.access.GatewayFailure
import it.lelloman.homelab.access.TunnelProxy
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import okhttp3.Dns
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.io.IOException
import java.net.InetAddress
import java.net.Proxy
import java.net.URI
import java.util.concurrent.TimeUnit

internal class RemoteAccessFailure(message: String) : IOException(message)

/** App-only tunnel. The canonical service URL and its TLS verification never change. */
internal class HomelabGateway(context: Context) : AutoCloseable {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val vault = SessionVault(context, "gateway_session")
    private var saved = vault.read() ?: JSONObject()
    private val outer = builder().proxy(Proxy.NO_PROXY).build()
    private val api = GatewayApi(GATEWAY, outer)
    @Volatile private var credential: GatewayCredential? = runCatching {
        saved.optString("credential").takeIf { it.isNotEmpty() }?.let { api.decodeCredential(it.toByteArray()) }
    }.getOrNull()
    var enrolled by mutableStateOf(credential != null); private set
    var pending by mutableStateOf(saved.has("pending")); private set
    var route by mutableStateOf("Not checked"); private set
    private val requests = Mutex()
    @Volatile private var denied = false
    private val tunnel by lazy {
        TunnelProxy(GATEWAY, mapOf("$HOST:443" to "talia"), { credential },
            outer.newBuilder().callTimeout(0, TimeUnit.SECONDS).readTimeout(0, TimeUnit.SECONDS).build()) {
            if (it is GatewayFailure && it.status in listOf(401, 403)) denied = true
        }
    }
    private var tunnelCreated = false
    private fun proxyClient(): OkHttpClient {
        tunnelCreated = true
        return tunnel.configure(builder()).build()
    }
    private val remote by lazy { proxyClient() }

    private fun builder() = OkHttpClient.Builder().followRedirects(false).followSslRedirects(false)
        .retryOnConnectionFailure(false).connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS).callTimeout(30, TimeUnit.SECONDS)

    private suspend fun lanClient(): OkHttpClient? = withContext(Dispatchers.IO) {
        val wifi = connectivity.allNetworks.firstOrNull {
            connectivity.getNetworkCapabilities(it)?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
        } ?: return@withContext null
        val client = builder().proxy(Proxy.NO_PROXY).socketFactory(wifi.socketFactory)
            .dns(object : Dns {
                override fun lookup(hostname: String): List<InetAddress> =
                    if (hostname == HOST) listOf(InetAddress.getByName("192.168.1.101")) else wifi.getAllByName(hostname).toList()
            }).callTimeout(2, TimeUnit.SECONDS).build()
        val reachable = try {
            client.newCall(Request.Builder().url("$HOME/healthz").build()).execute().use { it.code == 200 }
        } catch (_: IOException) { false }
        if (reachable) client.newBuilder().callTimeout(30, TimeUnit.SECONDS).build()
        else { client.connectionPool.evictAll(); null }
    }

    suspend fun requireHome() {
        val client = lanClient() ?: throw RemoteAccessFailure("Connect to home Wi-Fi to sign in.")
        withContext(NonCancellable + Dispatchers.IO) { client.connectionPool.evictAll() }
    }

    suspend fun begin(): String {
        requireHome()
        val enrollment = withContext(Dispatchers.IO) { api.begin("talia") }
        saved.put("pending", String(api.encodePending(enrollment))).put("expires", System.currentTimeMillis() + 600_000)
        vault.write(saved); pending = true
        return enrollment.authorizationUrl
    }

    suspend fun complete(callback: URI) {
        if (!pending || System.currentTimeMillis() >= saved.optLong("expires")) {
            cancel(); throw RemoteAccessFailure("Remote access setup expired. Start again on home Wi-Fi.")
        }
        val enrollment = api.decodePending(saved.getString("pending").toByteArray())
        val next = withContext(Dispatchers.IO) { api.complete(enrollment, callback, CALLBACK, "${Build.MANUFACTURER} ${Build.MODEL}") }
        // Preserve a successfully redeemed credential even if the catalog request fails.
        saved = JSONObject().put("credential", String(api.encodeCredential(next)))
        vault.write(saved); credential = next; pending = false; enrolled = true; denied = false
        val services = withContext(Dispatchers.IO) { api.services(next) }
        if (services["talia"]?.authority != "$HOST:443") throw RemoteAccessFailure("Talìa is not configured on the access gateway yet.")
    }

    fun cancel() {
        saved.remove("pending"); saved.remove("expires"); vault.write(saved); pending = false
    }

    suspend fun revoke() {
        credential?.let { withContext(Dispatchers.IO) { api.revoke(it) } }
        // On failure keep the credential so revocation can be retried.
        credential = null; enrolled = false; pending = false; denied = false
        saved = JSONObject(); vault.clear()
        if (tunnelCreated) tunnel.disconnect()
    }

    suspend fun request(server: String, path: String, token: String?, body: JSONObject?, headers: Map<String, String> = emptyMap()): JSONObject = requests.withLock {
        val managed = server == HOME
        val lan = if (managed) lanClient() else null
        val client = when {
            !managed -> outer
            lan != null -> { route = "Home network"; lan }
            credential == null || denied -> {
                route = "Remote access needs authorization"
                throw RemoteAccessFailure("Enable remote access on home Wi-Fi, then try again.")
            }
            else -> { route = "Secure tunnel"; remote }
        }
        try {
            withContext(Dispatchers.IO) {
                val request = Request.Builder().url(server + path).header("Accept", "application/json")
                token?.let { request.header("Authorization", "Bearer $it") }
                headers.forEach { (name, value) -> request.header(name, value) }
                body?.let { request.post(it.toString().toRequestBody("application/json".toMediaType())) }
                client.newCall(request.build()).execute().use { response ->
                    if (!response.isSuccessful) throw ApiFailure(response.code)
                    if (response.code == 202) return@withContext JSONObject().put("pending", true)
                    if (response.code == 204) return@withContext JSONObject()
                    val source = response.body?.source() ?: throw IOException("Empty response")
                    source.request(1_048_577)
                    if (source.buffer.size > 1_048_576) throw IOException("Response too large")
                    JSONObject(source.readUtf8())
                }
            }
        } catch (error: IOException) {
            if (managed && lan == null && denied) throw RemoteAccessFailure("Remote access was revoked. Enable it again on home Wi-Fi.")
            throw error
        } finally { withContext(NonCancellable + Dispatchers.IO) { lan?.connectionPool?.evictAll() } }
    }

    override fun close() {
        if (tunnelCreated) { tunnel.close(); remote.connectionPool.evictAll(); remote.dispatcher.executorService.shutdown() }
        outer.connectionPool.evictAll(); outer.dispatcher.executorService.shutdown()
    }
    companion object {
        const val HOST = "talia.lan.lelloman.com"
        const val HOME = "https://$HOST"
        const val GATEWAY = "https://access.lelloman.com"
        val CALLBACK: URI = URI("com.lelloman.talia://gateway/callback")
        fun isCallback(uri: URI) = uri.scheme == CALLBACK.scheme && uri.rawAuthority == CALLBACK.rawAuthority && uri.path == CALLBACK.path
    }
}
