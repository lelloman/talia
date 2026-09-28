package com.lelloman.talia

import android.app.Application
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.compose.runtime.*
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.URI
import java.net.HttpURLConnection
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

internal class SessionVault(context: Context, name: String = "native_session") {
    private val preferences = context.getSharedPreferences(name, Context.MODE_PRIVATE)
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        return (store.getKey("talia.session", null) as? SecretKey) ?: KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder("talia.session", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    fun read(): JSONObject? = runCatching {
        val bytes = Base64.decode(preferences.getString("sealed", null) ?: return null, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        JSONObject(String(cipher.doFinal(bytes.copyOfRange(12, bytes.size)), Charsets.UTF_8))
    }.getOrElse { clear(); null }
    fun write(value: JSONObject) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val bytes = cipher.iv + cipher.doFinal(value.toString().toByteArray(Charsets.UTF_8))
        check(preferences.edit().putString("sealed", Base64.encodeToString(bytes, Base64.NO_WRAP)).commit())
    }
    fun clear() { preferences.edit().clear().commit() }
}
internal class ApiFailure(val status: Int) : Exception()
internal object NativeApi {
    fun origin(input: String): String {
        val uri = URI(input.trim())
        require(uri.scheme == "https" && !uri.host.isNullOrEmpty() && uri.userInfo == null && uri.rawQuery == null && uri.fragment == null && (uri.path.isNullOrEmpty() || uri.path == "/"))
        return uri.toString().trimEnd('/')
    }
    suspend fun request(server: String, path: String, token: String? = null, body: JSONObject? = null): JSONObject = withContext(Dispatchers.IO) {
        val connection = URI(server + path).toURL().openConnection() as HttpURLConnection
        try {
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 10_000; connection.readTimeout = 20_000
            connection.setRequestProperty("Accept", "application/json")
            token?.let { connection.setRequestProperty("Authorization", "Bearer $it") }
            if (body != null) {
                connection.requestMethod = "POST"; connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            }
            val status = connection.responseCode
            if (status !in 200..299) throw ApiFailure(status)
            if (status == 202) return@withContext JSONObject().put("pending", true)
            if (status == 204) return@withContext JSONObject()
            val bytes = connection.inputStream.use { input ->
                val output = ByteArrayOutputStream()
                val buffer = ByteArray(8192)
                while (true) {
                    val count = input.read(buffer)
                    if (count < 0) break
                    check(output.size() + count <= 1_048_576)
                    output.write(buffer, 0, count)
                }
                output.toByteArray()
            }
            check(bytes.size <= 1_048_576)
            JSONObject(String(bytes, Charsets.UTF_8))
        } finally { connection.disconnect() }
    }
}
internal class NativeConnection @JvmOverloads constructor(
    application: Application,
    private val request: (suspend (String, String, String?, JSONObject?) -> JSONObject)? = null
) : AndroidViewModel(application) {
    val gateway by lazy { HomelabGateway(application) }
    private suspend fun call(server: String, path: String, token: String?, body: JSONObject?): JSONObject =
        request?.invoke(server, path, token, body) ?: gateway.request(server, path, token, body)
    override fun onCleared() { if (request == null) CoroutineScope(Dispatchers.IO).launch { gateway.close() } }
    private val vault = SessionVault(application)
    private var saved = vault.read() ?: JSONObject()
    var server by mutableStateOf(saved.optString("server", "https://talia.lan.lelloman.com")); private set
    var signedIn by mutableStateOf(saved.has("token")); private set
    var pending by mutableStateOf(saved.has("attempt")); private set
    var name by mutableStateOf("Not signed in"); private set
    var overview by mutableStateOf<JSONObject?>(null); private set
    var busy by mutableStateOf(false); private set
    var message by mutableStateOf<String?>(null); private set
    var forbidden by mutableStateOf(false); private set
    private var operation: Job? = null
    private fun launch(block: suspend () -> Unit) {
        if (operation?.isActive == true) return
        operation = viewModelScope.launch {
            busy = true
            try { block() }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                // Class names only: never log response bodies, URLs or credentials.
                android.util.Log.w("TaliaConnection", "Request failed: ${error.javaClass.name}; cause=${error.cause?.javaClass?.name}")
                if (error is RemoteAccessFailure) {
                    message = error.message
                } else if (error is it.lelloman.homelab.access.GatewayFailure) {
                    message = "Remote access authorization failed. Try setup again on home Wi-Fi."
                } else if (error is ApiFailure && error.status == 401) {
                    clear(); message = "Your session expired. Sign in again."
                } else if (error is ApiFailure && error.status == 403) {
                    overview = null; forbidden = true; message = "Overview requires administrator access."
                } else {
                    message = if (error is ApiFailure && error.status in listOf(404, 405)) "This server does not support the native app yet." else "Could not connect. Check your connection and try again."
                }
            } finally { busy = false }
        }
    }
    fun signIn(address: String, openBrowser: (String) -> Unit) {
        val valid = runCatching { NativeApi.origin(address) }.getOrElse { message = "Enter an HTTPS server address without a path."; return }
        launch {
            if (request == null && valid == HomelabGateway.HOME) {
                gateway.requireHome()
                if (!gateway.enrolled) {
                    clear(); server = valid; message = null
                    openBrowser(gateway.begin())
                    return@launch
                }
            }
            startNative(valid, openBrowser)
        }
    }
    private suspend fun startNative(valid: String, openBrowser: (String) -> Unit) {
        clear(); server = valid; message = null
        val verifier = Base64.encodeToString(ByteArray(32).also { SecureRandom().nextBytes(it) }, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
        val challenge = Base64.encodeToString(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)), Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
        val response = call(server, "/native/start", null, JSONObject().put("challenge", challenge))
        val attempt = response.getString("attempt")
        require(Regex("[A-Za-z0-9_-]{43}").matches(attempt))
        saved = JSONObject().put("server", server).put("attempt", attempt).put("verifier", verifier).put("expires", System.currentTimeMillis() + 600_000)
        vault.write(saved); pending = true
        openBrowser("$server/native/authorize?attempt=$attempt")
    }
    fun enableRemote(openBrowser: (String) -> Unit) = launch {
        message = null
        openBrowser(gateway.begin())
    }
    fun completeGateway(callback: URI, openBrowser: (String) -> Unit) {
        viewModelScope.launch {
            operation?.join()
            this@NativeConnection.launch { finishGateway(callback, openBrowser) }
        }
    }
    private suspend fun finishGateway(callback: URI, openBrowser: (String) -> Unit) {
        if (!HomelabGateway.isCallback(callback)) return
        gateway.complete(callback); message = null
        if (!signedIn) {
            gateway.requireHome()
            startNative(HomelabGateway.HOME, openBrowser)
        }
    }
    fun cancelGateway() { operation?.cancel(); gateway.cancel(); message = null }
    fun disableRemote() = launch { gateway.revoke(); message = null }
    fun refresh() = launch {
        if (pending) {
            if (System.currentTimeMillis() >= saved.optLong("expires")) { clear(); message = "Sign-in timed out. Please try again."; return@launch }
            val response = call(server, "/native/exchange", null, JSONObject().put("attempt", saved.getString("attempt")).put("verifier", saved.getString("verifier")))
            if (response.optBoolean("pending")) return@launch
            val token = response.getString("token"); require(Regex("n\\.[A-Za-z0-9_-]{43}").matches(token))
            saved = JSONObject().put("server", server).put("token", token)
            vault.write(saved); pending = false; signedIn = true
        }
        if (!signedIn) return@launch
        val identity = call(server, "/native/session", saved.getString("token"), null)
        name = identity.getString("name")
        overview = call(server, "/native/overview", saved.getString("token"), null)
        forbidden = false; message = null
    }
    fun cancelSignIn() { operation?.cancel(); clear(); message = null }
    fun signOut() = launch {
        call(server, "/native/logout", saved.getString("token"), JSONObject())
        clear(); message = null
        if (request == null && gateway.enrolled) gateway.revoke()
    }
    private fun clear() {
        vault.clear(); saved = JSONObject(); signedIn = false; pending = false
        overview = null; name = "Not signed in"; forbidden = false
    }
}
