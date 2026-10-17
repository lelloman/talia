package com.lelloman.talia

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.util.UUID

internal class AlertModel(private val call: suspend (JSONObject) -> JSONObject) {
    var alerts by mutableStateOf<List<JSONObject>>(emptyList()); private set
    var loaded by mutableStateOf(false); private set
    var busy by mutableStateOf(false); private set
    var message by mutableStateOf<String?>(null); private set
    var forbidden by mutableStateOf(false); private set
    private suspend fun rpc(op: String, args: JSONObject = JSONObject()): JSONObject {
        val result = call(JSONObject().put("op", op).put("args", args))
        check(!result.has("error")) { result.optString("error") }
        return result
    }
    private fun failure(e: Exception) {
        forbidden = e is ApiFailure && e.status == 403
        message = when {
            forbidden -> "Alerts requires an administrator account."
            e is ApiFailure && e.status == 401 -> "Your session expired. Sign in again."
            e is ApiFailure && e.status in listOf(404, 405) -> "This server needs the alerts update."
            e is ApiFailure && e.status == 409 -> "This alert changed. Refresh and review it before acknowledging again."
            else -> "Could not confirm the latest alert state. Check your connection and refresh."
        }
    }
    private suspend fun load() {
        val list = rpc("snapshot").getJSONArray("alerts")
        alerts = (0 until list.length()).map { list.getJSONObject(it) }
            .sortedWith(compareByDescending<JSONObject> { it.getBoolean("active") }.thenByDescending { it.optLong("updated_at") })
        loaded = true; forbidden = false
    }
    suspend fun refresh() {
        if (busy) return
        busy = true
        try { load(); message = null }
        catch (e: CancellationException) { throw e }
        catch (e: Exception) { failure(e) }
        finally { busy = false }
    }
    suspend fun acknowledge(alert: JSONObject) {
        if (busy) return
        busy = true
        try {
            rpc("acknowledge", JSONObject().put("key", alert.getString("key"))
                .put("occurrence", alert.getLong("occurrence")).put("expected", alert.getLong("revision"))
                .put("requestId", UUID.randomUUID().toString()))
            load()
            message = "Acknowledged. Reminders for this occurrence have stopped."
        } catch (e: CancellationException) { throw e }
        catch (e: Exception) { failure(e) }
        finally { busy = false }
    }
}

@Composable
internal fun AlertsScreen(connection: NativeConnection, modifier: Modifier = Modifier, settings: () -> Unit) {
    if (!connection.signedIn) {
        Column(modifier.padding(24.dp)) {
            Text("Sign in to view alerts.")
            Button(onClick = settings) { Text("Open Settings") }
        }
        return
    }
    val model = connection.alerts
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var showResolved by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(model, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) { model.refresh(); delay(30000) }
        }
    }
    LazyColumn(modifier, contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        item {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Alerts", style = MaterialTheme.typography.headlineSmall)
                Text("Acknowledge an alert to stop its reminders. It stays active until the condition recovers. New incidents can notify again.")
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    OutlinedButton(enabled = !model.busy, onClick = { scope.launch { model.refresh() } }) { Text("Refresh") }
                    TextButton(onClick = { showResolved = !showResolved }) { Text(if (showResolved) "Hide resolved" else "Show resolved") }
                }
                if (model.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
                model.message?.let { Text(it) }
            }
        }
        if (!model.forbidden) {
            val visible = model.alerts.filter { showResolved || it.getBoolean("active") }
            if (model.loaded && visible.isEmpty()) item { Text(if (showResolved) "No alerts" else "No active alerts") }
            items(visible, key = { it.getString("key") }) { alert ->
                val active = alert.getBoolean("active")
                val acknowledgement = alert.optJSONObject("acknowledgement")
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(alert.getString("key"), style = MaterialTheme.typography.titleMedium)
                        Text("${alert.getString("severity")} · ${if (active) "Active" else "Resolved"}")
                        Text(alert.getString("message"))
                        if (acknowledgement != null) Text("Acknowledged by ${acknowledgement.optString("actor")}")
                        else if (active) Button(enabled = !model.busy, onClick = { scope.launch { model.acknowledge(alert) } }) { Text("Acknowledge") }
                    }
                }
            }
        }
    }
}
