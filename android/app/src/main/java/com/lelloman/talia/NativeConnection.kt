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
    var notificationsMessage by mutableStateOf<String?>(null); private set
    var notificationsEnabled by mutableStateOf(saved.has("notificationSubscription")); private set
    fun enableNotifications() = launch {
        check(BuildConfig.STORE_CERTIFICATES.isNotBlank()) { "Store signing certificate missing from this build" }
        val identity = call(server, "/native/session", saved.getString("token"), null).getString("subject")
        val split = identity.lastIndexOf('#'); check(split > 0)
        val client = TaliaNotifications.client(getApplication())
        client.beginSession(identity.substring(0, split), identity.substring(split + 1))
        val proof = client.enrollment()
        val enrollment = call(server, "/native/notifications", saved.getString("token"), JSONObject().put("op", "enroll").put("proof", proof.getString("proof")))
        val subscription = enrollment.getString("subscription_id")
        client.confirm(subscription)
        saved.put("notificationSubscription", subscription); vault.write(saved)
        notificationsEnabled = true; notificationsMessage = "Notifications enabled through LelloStore."
    }
    fun disableNotifications() = launch {
        TaliaNotifications.client(getApplication()).endSession()
        notificationsEnabled = false
        val id = saved.optString("notificationSubscription")
        if (id.isNotEmpty()) call(server, "/native/notifications", saved.getString("token"), JSONObject().put("op", "disable").put("subscription_id", id))
        saved.remove("notificationSubscription"); vault.write(saved)
        notificationsMessage = "Notifications disabled."
    }
    fun openNotification(report: String, run: String) {
        // Opening a notification only selects a destination; normal APIs still authorize its content.
        if (report.isNotEmpty()) { reportSelected = report; reportRunSelected = run; refreshReports() }
    }
    var signedIn by mutableStateOf(saved.has("token")); private set
    var pending by mutableStateOf(saved.has("attempt")); private set
    var name by mutableStateOf("Not signed in"); private set
    var overview by mutableStateOf<JSONObject?>(null); private set
    var busy by mutableStateOf(false); private set
    var message by mutableStateOf<String?>(null); private set
    var forbidden by mutableStateOf(false); private set
    var reportDefinitions by mutableStateOf<org.json.JSONArray?>(null); private set
    var reportSchedule by mutableStateOf<JSONObject?>(null); private set
    var scheduleMessage by mutableStateOf<String?>(null); private set
    var pendingSchedule by mutableStateOf(saved.has("scheduleRequest")); private set
    fun saveReportSchedule(enabled: Boolean, schedule: JSONObject?, version: Long) {
        if (reportsBusy || pendingSchedule) return
        val args = JSONObject().put("id", reportSelected).put("enabled", enabled)
            .put("schedule", schedule ?: JSONObject.NULL).put("expected", version)
            .put("requestId", java.util.UUID.randomUUID().toString())
        saved.put("scheduleRequest", args); vault.write(saved); pendingSchedule = true
        retryReportSchedule()
    }
    fun retryReportSchedule() = reportTask {
        val args = saved.optJSONObject("scheduleRequest") ?: return@reportTask
        try {
            val result = reportCall("schedule_save", args)
            if (reportSelected == args.getString("id")) reportSchedule = result
            saved.remove("scheduleRequest"); vault.write(saved); pendingSchedule = false
            scheduleMessage = "Schedule saved."
        } catch (failure: ApiFailure) {
            if (failure.status in listOf(400, 403, 409)) {
                saved.remove("scheduleRequest"); vault.write(saved); pendingSchedule = false
                scheduleMessage = if (failure.status == 409) "Schedule changed elsewhere. Reload before editing again."
                    else "Schedule rejected. Check the time, timezone and delivery destinations, then reload."
            } else scheduleMessage = "Save not confirmed. Check the saved request before editing again."
            throw failure
        } catch (failure: Exception) {
            scheduleMessage = "Save not confirmed. Check the saved request before editing again."
            throw failure
        }
    }
    var reportHistory by mutableStateOf(org.json.JSONArray()); private set
    var reportDetail by mutableStateOf<JSONObject?>(null); private set
    var reportSelected by mutableStateOf(saved.optString("reportSelected", "")); private set
    var reportRunSelected by mutableStateOf(saved.optString("reportRunSelected", "")); private set
    var reportsBusy by mutableStateOf(false); private set
    var reportsMessage by mutableStateOf<String?>(null); private set
    var reportsForbidden by mutableStateOf(false); private set
    var reportsNext by mutableStateOf<String?>(null); private set
    var pendingReport by mutableStateOf(saved.optJSONObject("reportRequest")?.optString("id")); private set
    private var reportOperation: Job? = null
    private fun reportTask(block: suspend () -> Unit) {
        if (!signedIn || reportOperation?.isActive == true) return
        reportOperation = viewModelScope.launch {
            reportsBusy = true
            try { block(); reportsMessage = null; reportsForbidden = false }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                reportsMessage = when {
                    error is ApiFailure && error.status == 401 -> { clear(); "Your session expired. Sign in again." }
                    error is ApiFailure && error.status == 403 -> {
                        reportDefinitions = null; reportHistory = org.json.JSONArray(); reportDetail = null; reportSchedule = null; scheduleMessage = null
                        reportsForbidden = true; "Reports require administrator access."
                    }
                    error is ApiFailure && error.status in listOf(404, 405) -> "This server needs the Reports update."
                    error is ApiFailure && error.status == 409 -> "Schedule changed elsewhere. Reload and review the current settings."
                    error is ApiFailure && error.status == 400 -> "This request was rejected. Another run may already be active, or the report is unavailable."
                    error is RemoteAccessFailure -> error.message
                    else -> "Could not refresh Reports. Check your connection and try again."
                }
            } finally { reportsBusy = false }
        }
    }
    private suspend fun reportCall(op: String, args: JSONObject = JSONObject()): JSONObject =
        call(server, "/native/reports", saved.getString("token"), JSONObject().put("op", op).put("args", args))
    private fun saveReportSelection() {
        saved.put("reportSelected", reportSelected).put("reportRunSelected", reportRunSelected)
        vault.write(saved)
    }
    fun selectReport(id: String) {
        if (reportsBusy) return
        reportSelected = id; reportRunSelected = ""; reportDetail = null; reportSchedule = null; scheduleMessage = null
        reportHistory = org.json.JSONArray(); reportsNext = null; saveReportSelection(); refreshReports()
    }
    fun selectReportRun(id: String) {
        if (reportsBusy) return
        reportRunSelected = id; reportDetail = null; saveReportSelection(); refreshReports()
    }
    fun reportsBack() {
        if (reportsBusy) return
        if (reportRunSelected.isNotEmpty()) reportRunSelected = "" else reportSelected = ""
        reportDetail = null; saveReportSelection(); refreshReports()
    }
    var reportSort by mutableStateOf("newest"); private set
    var reportFilter by mutableStateOf("all"); private set
    fun filterReportHistory(sort: String, status: String) {
        if (reportsBusy) return
        reportSort = sort; reportFilter = status
        reportHistory = org.json.JSONArray(); reportsNext = null
        refreshReports()
    }
    private fun historyArgs() = JSONObject().put("report", reportSelected).put("limit", 20)
        .put("sort", reportSort).put("status", reportFilter)
    fun refreshReports() = reportTask {
        when {
            reportRunSelected.isNotEmpty() -> reportDetail = reportCall("run_get", JSONObject().put("id", reportRunSelected)).getJSONObject("run")
            reportSelected.isNotEmpty() -> {
                reportSchedule = reportCall("schedule_get", JSONObject().put("id", reportSelected))
                val value = reportCall("runs", historyArgs())
                reportHistory = value.getJSONArray("runs")
                reportsNext = if (value.isNull("next_before")) null else value.getString("next_before")
            }
            else -> reportDefinitions = reportCall("list").getJSONArray("definitions")
        }
    }
    fun moreReportRuns() = reportTask {
        val cursor = reportsNext ?: return@reportTask
        val value = reportCall("runs", historyArgs().put("before", cursor))
        val combined = org.json.JSONArray(reportHistory.toString())
        val more = value.getJSONArray("runs")
        for (i in 0 until more.length()) combined.put(more.getJSONObject(i))
        reportHistory = combined
        reportsNext = if (value.isNull("next_before")) null else value.getString("next_before")
    }
    fun runReport() = reportTask {
        // Persist before dispatch. Explicit retry, including after restart, reuses the same request.
        val args = saved.optJSONObject("reportRequest") ?: JSONObject().put("id", reportSelected)
            .put("send", false).put("requestId", java.util.UUID.randomUUID().toString()).also {
                saved.put("reportRequest", it); vault.write(saved); pendingReport = reportSelected
            }
        val response = try { reportCall("run", args) } catch (failure: ApiFailure) {
            if (failure.status == 400 || failure.status == 403) {
                saved.remove("reportRequest"); vault.write(saved); pendingReport = null
            }
            throw failure
        }
        reportSelected = args.getString("id"); reportRunSelected = response.getString("run_id")
        saved.remove("reportRequest"); pendingReport = null; saveReportSelection()
        reportDetail = reportCall("run_get", JSONObject().put("id", reportRunSelected)).getJSONObject("run")
    }
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
                    overview = null; forbidden = true; reportDefinitions = null; reportHistory = org.json.JSONArray(); reportDetail = null; reportSchedule = null; scheduleMessage = null; message = "Overview requires administrator access."
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
        TaliaNotifications.client(getApplication()).endSession()
        call(server, "/native/logout", saved.getString("token"), JSONObject())
        clear(); message = null
        if (request == null && gateway.enrolled) gateway.revoke()
    }
    private fun clear() {
        TaliaNotifications.client(getApplication()).endSession()
        notificationsEnabled = false; notificationsMessage = null
        reportOperation?.cancel()
        reportDefinitions = null; reportHistory = org.json.JSONArray(); reportDetail = null; reportSchedule = null; scheduleMessage = null
        reportSelected = ""; reportRunSelected = ""; reportsNext = null; pendingReport = null; pendingSchedule = false; reportsMessage = null; reportsForbidden = false
        vault.clear(); saved = JSONObject(); signedIn = false; pending = false
        overview = null; name = "Not signed in"; forbidden = false
    }
}
