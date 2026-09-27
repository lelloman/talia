package com.lelloman.talia

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import com.lelloman.lellodesign.LelloSection
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private fun time(value: Long): String = DateTimeFormatter.ofPattern("EEE dd MMM yyyy HH:mm z")
    .withZone(ZoneId.systemDefault()).format(Instant.ofEpochMilli(value))

@Composable
internal fun ConnectionPanel(connection: NativeConnection, openBrowser: (String) -> Unit) {
    var address by rememberSaveable { mutableStateOf(connection.server) }
    LelloSection("Connection") {
        if (connection.signedIn) {
            Text("Signed in as ${connection.name}")
            Text(connection.server)
            Button(onClick = connection::signOut, enabled = !connection.busy) { Text("Sign out") }
        } else if (connection.pending) {
            Text("Complete sign-in in your browser, then return to Talìa.")
            TextButton(onClick = connection::refresh, enabled = !connection.busy) { Text("Check sign-in") }
            TextButton(onClick = connection::cancelSignIn) { Text("Cancel sign-in") }
        } else {
            OutlinedTextField(value = address, onValueChange = { address = it }, label = { Text("Server address") }, singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri), enabled = !connection.busy, modifier = Modifier.fillMaxWidth())
            Button(onClick = { connection.signIn(address, openBrowser) }, enabled = !connection.busy) { Text("Sign in with LelloAuth") }
        }
        connection.message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (connection.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    }
}

@Composable
internal fun Overview(connection: NativeConnection, openBrowser: (String) -> Unit) {
    if (!connection.signedIn) { ConnectionPanel(connection, openBrowser); return }
    LelloSection("Overview") {
        Text("${connection.name} · ${connection.server}")
        TextButton(onClick = connection::refresh, enabled = !connection.busy) { Text("Refresh") }
        if (connection.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        connection.message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    }
    val data = connection.overview ?: run {
        if (!connection.forbidden && !connection.busy) Text("Connect to load service status and recent reports.")
        return
    }
    Text("Last synced ${time(data.getLong("fetchedAt"))}")
    if (connection.message != null) Text("Showing the last successful response. It may be out of date.")
    LelloSection("Service metrics") {
        Text("Checks whether Prometheus can collect each service’s metrics; this is not a full application health check.")
        val stale = data.optBoolean("stale") || (!data.isNull("sampledAt") && System.currentTimeMillis() - data.getLong("sampledAt") > 120_000)
        if (!data.isNull("sampledAt")) Text("Sampled ${time(data.getLong("sampledAt"))}")
        if (stale) Text("Samples are stale or unavailable.", color = MaterialTheme.colorScheme.error)
        val services = data.getJSONArray("services")
        if (services.length() == 0) Text("No service samples are available yet.")
        for (index in 0 until services.length()) {
            val service = services.getJSONObject(index)
            val status = when (service.optString("status")) { "up" -> "Reachable"; "down" -> "Unreachable"; else -> "Unknown" }
            Text("${service.getString("name")} — $status${if (stale) " (stale)" else ""}")
        }
    }
    LelloSection("Recent reports") {
        val reports = data.getJSONArray("reports")
        if (reports.length() == 0) Text("No reports have run yet.")
        for (index in 0 until reports.length()) {
            ReportSummary(reports.getJSONObject(index))
            HorizontalDivider()
        }
    }
}
@Composable
private fun ReportSummary(report: JSONObject) {
    Text(if (report.isNull("title")) report.getString("report") else report.getString("title"), style = MaterialTheme.typography.titleMedium)
    Text("${report.getString("status")} · ${time(report.getLong("created"))}")
    if (!report.isNull("summary")) Text(report.getString("summary"))
    Text("Run ${report.getString("id")}", style = MaterialTheme.typography.bodySmall)
}
