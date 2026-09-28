package com.lelloman.talia

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.lelloman.lellodesign.*
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private fun time(value: Long): String = DateTimeFormatter.ofPattern("EEE dd MMM · HH:mm z")
    .withZone(ZoneId.systemDefault()).format(Instant.ofEpochMilli(value))

@Composable
internal fun Muted(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun StatusLabel(label: String, tone: LelloTone) {
    val palette = LocalLelloPalette.current
    val key = tone.name.lowercase(java.util.Locale.ROOT)
    Surface(shape = MaterialTheme.shapes.small, color = palette["$key-container"], contentColor = palette["on-$key-container"]) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Box(Modifier.size(6.dp).background(LocalContentColor.current, CircleShape))
            Text(label, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
internal fun ConnectionPanel(connection: NativeConnection, openBrowser: (String) -> Unit) {
    var address by rememberSaveable(connection.server) { mutableStateOf(connection.server) }
    var editServer by rememberSaveable { mutableStateOf(connection.server != HomelabGateway.HOME) }
    LelloSettingsSection(if (connection.signedIn) "Account" else "Connect to Talìa") {
        if (connection.gateway.pending || connection.pending) {
            LelloAlert("Finish signing in", tone = LelloTone.Info, modifier = Modifier.fillMaxWidth()) {
                Text(if (connection.gateway.pending) "Authorize remote access in your browser. Talìa will continue when you return." else "Complete sign-in in your browser, then return to Talìa.")
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (connection.pending) LelloOutlinedButton(connection::refresh, enabled = !connection.busy) { Text("Check sign-in") }
                LelloTextButton(if (connection.gateway.pending) connection::cancelGateway else connection::cancelSignIn) { Text("Cancel") }
            }
        } else if (connection.signedIn) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                LelloAvatar(connection.name)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(connection.name, style = MaterialTheme.typography.titleMedium)
                    Muted("Signed in with LelloAuth")
                }
            }
            LelloTextButton(connection::signOut, enabled = !connection.busy) { Text("Sign out", color = MaterialTheme.colorScheme.error) }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Muted("Your services and reports, together. Sign in on home Wi-Fi to get started.")
                if (editServer) {
                    LelloTextField(address, { address = it }, { Text("Server address") }, modifier = Modifier.fillMaxWidth(),
                        enabled = !connection.busy, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri))
                } else {
                    Text(if (address == HomelabGateway.HOME) "Home server" else "Server", style = MaterialTheme.typography.titleSmall)
                    Muted(address)
                }
                LelloButton({ connection.signIn(address, openBrowser) }, enabled = !connection.busy, modifier = Modifier.fillMaxWidth()) {
                    Text("Sign in with LelloAuth")
                }
                LelloTextButton({ editServer = !editServer }, enabled = !connection.busy) { Text(if (editServer) "Hide server address" else "Change server") }
            }
        }
        connection.message?.let { LelloAlert("Connection needs attention", tone = LelloTone.Warning, modifier = Modifier.fillMaxWidth()) { Text(it) } }
        if (connection.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    }
}

@Composable
internal fun ConnectionSettings(connection: NativeConnection, openBrowser: (String) -> Unit) {
    if (!connection.signedIn && !connection.gateway.enrolled) return
    LelloSettingsSection("Connection") {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(if (connection.server == HomelabGateway.HOME) "Home server" else "Server", style = MaterialTheme.typography.titleMedium)
            SelectionContainer { Muted(connection.server) }
        }
        if (connection.server == HomelabGateway.HOME) {
            LelloCard(Modifier.fillMaxWidth()) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Icon(Icons.Default.Lock, null, tint = MaterialTheme.colorScheme.primary)
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Access away from home", style = MaterialTheme.typography.titleMedium)
                        Muted(if (connection.gateway.enrolled) "Enabled · ${connection.gateway.route}" else "Set up once on home Wi-Fi")
                    }
                }
                Muted("Talìa switches automatically between your home network and a secure connection when you’re away.")
                if (!connection.gateway.pending) {
                    LelloOutlinedButton({ connection.enableRemote(openBrowser) }, enabled = !connection.busy) {
                        Text(if (connection.gateway.enrolled) "Renew authorization" else "Enable remote access")
                    }
                    if (connection.gateway.enrolled) LelloTextButton(connection::disableRemote, enabled = !connection.busy) { Text("Disable remote access") }
                }
            }
        }
    }
}

@Composable
internal fun Overview(connection: NativeConnection, openSettings: () -> Unit) {
    if (!connection.signedIn) {
        LelloState(
            title = if (connection.pending || connection.gateway.pending) "You’re almost connected" else "Your home, at a glance",
            description = if (connection.pending || connection.gateway.pending) "Finish signing in to see your services and recent reports." else "Keep an eye on your services and catch up on reports from wherever you are.",
            modifier = Modifier.fillMaxWidth(),
            icon = { androidx.compose.foundation.Image(androidx.compose.ui.res.painterResource(R.drawable.ic_talia), null, Modifier.size(80.dp)) },
            action = { LelloButton(openSettings) { Text(if (connection.pending || connection.gateway.pending) "Continue setup" else "Connect your server") } },
        )
        connection.message?.let { LelloAlert("Sign-in required", modifier = Modifier.fillMaxWidth(), tone = LelloTone.Warning) { Text(it) } }
        return
    }
    OverviewContent(connection.overview, connection.busy, connection.message, connection.forbidden,
        if (connection.server == HomelabGateway.HOME) connection.gateway.route else "Direct connection", connection::refresh)
}

@Composable
internal fun OverviewContent(data: JSONObject?, busy: Boolean, message: String?, forbidden: Boolean, route: String, refresh: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Home at a glance", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
                Muted(route)
            }
            LelloOutlinedButton(refresh, enabled = !busy, contentPadding = PaddingValues(horizontal = 12.dp)) {
                Icon(Icons.Default.Refresh, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("Refresh")
            }
        }
        if (data != null) Text("Updated ${time(data.getLong("fetchedAt"))}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    }
    if (message != null) LelloAlert(if (forbidden) "Administrator access required" else "Couldn’t refresh", modifier = Modifier.fillMaxWidth(), tone = LelloTone.Warning) {
        Text(message)
        if (data != null) Text("Showing the last successful response.")
    }
    if (data == null) {
        if (!forbidden) LelloState(if (busy) "Loading your overview" else "No overview yet",
            if (busy) "Fetching service status and recent reports." else "Refresh when you’re connected to load your home’s latest status.", modifier = Modifier.fillMaxWidth())
        return
    }
    val services = data.getJSONArray("services")
    val entries = (0 until services.length()).map { services.getJSONObject(it) }
    val stale = data.optBoolean("stale") || data.isNull("sampledAt") || System.currentTimeMillis() - data.optLong("sampledAt") > 120_000
    val up = entries.count { it.optString("status") == "up" }
    val down = entries.count { it.optString("status") == "down" }
    val unknown = entries.size - up - down
    LelloCard(Modifier.fillMaxWidth()) {
        StatusLabel(when { stale -> "Status is out of date"; entries.isEmpty() -> "No service data"; down > 0 -> "Services need attention"; unknown > 0 -> "Some status is unknown"; else -> "All monitored services reachable" },
            if (stale || entries.isEmpty()) LelloTone.Warning else if (down > 0) LelloTone.Error else if (unknown > 0) LelloTone.Warning else LelloTone.Success)
        val counts = listOf(up to "Reachable", down to "Unreachable", unknown to "Unknown")
        if (androidx.compose.ui.platform.LocalDensity.current.fontScale > 1.2f) {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                counts.forEach { (count, label) ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(count.toString(), style = MaterialTheme.typography.headlineMedium)
                        Text(label, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        } else Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            counts.forEach { (count, label) ->
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(count.toString(), style = MaterialTheme.typography.headlineMedium)
                    Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Muted(if (stale) "Last recorded samples. Refresh to check for newer data." else "Based on Prometheus metrics collection.")
    }
    LelloSection("Services") {
        if (entries.isEmpty()) Muted("No services are reporting metrics yet.")
        else Column {
            entries.forEachIndexed { index, service ->
                val status = service.optString("status")
                val label = (if (stale) "Last seen: " else "") + when (status) { "up" -> "Reachable"; "down" -> "Unreachable"; else -> "Unknown" }
                val tone = if (stale || status !in listOf("up", "down")) LelloTone.Warning else if (status == "up") LelloTone.Success else LelloTone.Error
                if (androidx.compose.ui.platform.LocalDensity.current.fontScale > 1.2f) {
                    Column(Modifier.fillMaxWidth().padding(vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(service.getString("name"), style = MaterialTheme.typography.titleMedium)
                        StatusLabel(label, tone)
                    }
                } else Row(Modifier.fillMaxWidth().padding(vertical = 12.dp), verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(service.getString("name"), Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    StatusLabel(label, tone)
                }
                if (index < entries.lastIndex) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            }
        }
        Muted("Reachability shows whether metrics can be collected, not a full application health check.")
    }
    LelloSection("Recent reports") {
        val reports = data.getJSONArray("reports")
        if (reports.length() == 0) LelloState("No reports yet", "Completed runs will appear here.", modifier = Modifier.fillMaxWidth())
        for (index in 0 until reports.length()) {
            val report = reports.getJSONObject(index)
            key(report.getString("id")) { ReportSummary(report) }
        }
    }
}

@Composable
private fun ReportSummary(report: JSONObject) {
    var expanded by rememberSaveable(report.getString("id")) { mutableStateOf(false) }
    val status = report.getString("status")
    val tone = when (status) { "succeeded" -> LelloTone.Success; "failed" -> LelloTone.Error; "running", "pending" -> LelloTone.Info; else -> LelloTone.Warning }
    LelloCard(Modifier.fillMaxWidth()) {
        Text(if (report.isNull("title")) report.getString("report") else report.getString("title"), style = MaterialTheme.typography.titleMedium)
        StatusLabel(when (status) { "succeeded" -> "Run completed"; "failed" -> "Run failed"; else -> status.replaceFirstChar { it.uppercase() } }, tone)
        Text(time(report.getLong("created")), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (!report.isNull("summary")) Text(report.getString("summary"), style = MaterialTheme.typography.bodyMedium,
            maxLines = if (expanded) Int.MAX_VALUE else 4, overflow = TextOverflow.Ellipsis)
        LelloTextButton({ expanded = !expanded }) { Text(if (expanded) "Show less" else "View details") }
        if (expanded) SelectionContainer {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Run ID", style = MaterialTheme.typography.labelMedium)
                Muted(report.getString("id"))
            }
        }
    }
}
