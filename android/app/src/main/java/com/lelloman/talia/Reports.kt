package com.lelloman.talia

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.lelloman.lellodesign.*
import kotlinx.coroutines.delay
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

internal fun reportTime(ms: Long): String = DateTimeFormatter.ofPattern("EEE dd MMM yyyy HH:mm z")
    .format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))
internal fun reportActive(status: String) = status in listOf("queued", "running", "delivering")

@Composable
internal fun RunStatus(status: String) = StatusLabel(when (status) {
    "complete" -> "Run completed"
    "partial" -> "Completed with issues"
    "failed" -> "Run failed"
    "queued" -> "Queued"
    "running" -> "Running"
    "delivering" -> "Sending results"
    else -> status.replaceFirstChar { it.uppercase() }
}, when (status) {
    "complete" -> LelloTone.Success
    "failed" -> LelloTone.Error
    "partial" -> LelloTone.Warning
    else -> LelloTone.Info
})

@Composable
internal fun Reports(connection: NativeConnection, setup: () -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var sort by rememberSaveable { mutableStateOf("newest") }
    var filter by rememberSaveable { mutableStateOf("all") }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(connection, lifecycle, connection.signedIn, connection.reportSelected, connection.reportRunSelected) {
        if (connection.signedIn) lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            connection.refreshReports()
            while (true) {
                delay(5000)
                if (connection.reportRunSelected.isNotEmpty() &&
                    (connection.reportDetail == null || reportActive(connection.reportDetail!!.optString("status")))) connection.refreshReports()
            }
        }
    }
    BackHandler(connection.signedIn && connection.reportSelected.isNotEmpty()) { connection.reportsBack() }
    if (!connection.signedIn) {
        LelloState("Connect to view reports", "Sign in to browse reports, run checks and read results.",
            modifier = Modifier.fillMaxWidth(), action = { LelloButton(setup) { Text("Open Settings") } })
        return
    }
    if (connection.reportSelected.isNotEmpty()) LelloTextButton(connection::reportsBack, enabled = !connection.reportsBusy) {
        Text(if (connection.reportRunSelected.isEmpty()) "All reports" else "Back to history")
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Column(Modifier.weight(1f)) {
            Text(if (connection.reportSelected.isEmpty()) "Available reports" else connection.reportSelected,
                style = MaterialTheme.typography.headlineSmall)
            Muted(if (connection.reportRunSelected.isNotEmpty()) "Run details" else if (connection.reportSelected.isNotEmpty()) "Run history" else "Checks and summaries from your server")
        }
        LelloTextButton(connection::refreshReports, enabled = !connection.reportsBusy) { Text("Refresh") }
    }
    if (connection.reportsBusy) LinearProgressIndicator(Modifier.fillMaxWidth())
    connection.reportsMessage?.let { LelloAlert(it, tone = LelloTone.Warning) }
    if (connection.reportsForbidden) return
    connection.pendingReport?.takeIf { !connection.reportsBusy }?.let {
        LelloAlert("A run request needs confirmation", tone = LelloTone.Warning) { Text("The server may already have started $it. Check the request to recover its run without starting another.") }
        LelloButton(connection::runReport, enabled = !connection.reportsBusy) { Text("Check run request") }
    }
    when {
        connection.reportRunSelected.isNotEmpty() -> connection.reportDetail?.let { ReportRunDetails(it) }
        connection.reportSelected.isNotEmpty() -> {
            val definitions = connection.reportDefinitions
            val definition = (0 until (definitions?.length() ?: 0)).map { definitions!!.getJSONObject(it) }.find { it.getString("id") == connection.reportSelected }
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                LelloButton(connection::runReport, enabled = !connection.reportsBusy && connection.pendingReport == null && definition?.optBoolean("available", true) != false) { Text("Run report") }
                Muted("Saved in Talìa · No Telegram or email sent")
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ReportChoice("Sort", connection.reportSort, listOf("newest" to "Newest first", "oldest" to "Oldest first"), !connection.reportsBusy) {
                        connection.filterReportHistory(it, connection.reportFilter)
                    }
                    ReportChoice("Status", connection.reportFilter, historyFilters, !connection.reportsBusy) {
                        connection.filterReportHistory(connection.reportSort, it)
                    }
                }
                if (connection.reportHistory.length() == 0 && !connection.reportsBusy && connection.reportsMessage == null)
                    Muted(if (connection.reportFilter == "all") "No runs yet." else "No runs match this filter.")
                for (i in 0 until connection.reportHistory.length()) {
                    val run = connection.reportHistory.getJSONObject(i)
                    CompactReportRow(reportTime(run.getLong("created")), compactStatus(run.getString("status")), run.getString("status"), !connection.reportsBusy) {
                        connection.selectReportRun(run.getString("id"))
                    }
                }
                if (connection.reportsNext != null) LelloOutlinedButton(connection::moreReportRuns, enabled = !connection.reportsBusy) { Text("Load more runs") }
            }
        }
        else -> {
            val definitions = connection.reportDefinitions
            val reports = catalogReports(definitions, query, sort, filter)
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                LelloTextField(query, { query = it }, { Text("Search reports") }, modifier = Modifier.fillMaxWidth(),
                    suffix = { if (query.isNotEmpty()) LelloTextButton({ query = "" }) { Text("Clear") } })
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ReportChoice("Sort", sort, listOf("newest" to "Newest first", "oldest" to "Oldest first", "name" to "Name A–Z")) { sort = it }
                    ReportChoice("Filter", filter, listOf("all" to "All reports", "issues" to "Needs attention", "active" to "Running", "scheduled" to "Scheduled", "never" to "Never run")) { filter = it }
                }
                Muted("${reports.size} of ${definitions?.length() ?: 0} reports")
                if (reports.isEmpty() && !connection.reportsBusy && connection.reportsMessage == null)
                    Muted(if (definitions?.length() == 0) "No reports configured." else "No reports match your search or filter.")
                for (report in reports) {
                    val latest = report.optJSONObject("latest")
                    val status = latest?.optString("status") ?: "never"
                    val summary = if (!report.optBoolean("available")) "Needs configuration" else
                        "${compactStatus(status)} · " + (latest?.let { shortReportTime(it.getLong("created")) } ?: "${report.getInt("steps")} checks")
                    CompactReportRow(report.getString("id"), summary, status, !connection.reportsBusy) {
                        connection.selectReport(report.getString("id"))
                    }
                }
            }
        }
    }
}

private val historyFilters = listOf("all" to "All statuses", "active" to "Running", "complete" to "Completed", "issues" to "With issues", "failed" to "Failed")
private fun shortReportTime(ms: Long) = DateTimeFormatter.ofPattern("dd MMM · HH:mm").format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))
private fun compactStatus(status: String) = when (status) {
    "complete" -> "Completed"; "partial" -> "With issues"; "failed" -> "Failed"
    "queued" -> "Queued"; "running" -> "Running"; "delivering" -> "Sending"; "never" -> "Never run"; else -> status
}
internal fun catalogReports(data: org.json.JSONArray?, query: String, sort: String, filter: String): List<JSONObject> {
    val rows = (0 until (data?.length() ?: 0)).map { data!!.getJSONObject(it) }.filter { report ->
        val status = report.optJSONObject("latest")?.optString("status")
        report.getString("id").contains(query.trim(), ignoreCase = true) && when (filter) {
            "issues" -> status in listOf("failed", "partial") || !report.optBoolean("available", true)
            "active" -> status != null && reportActive(status)
            "scheduled" -> report.optBoolean("enabled") && report.optBoolean("scheduled")
            "never" -> status == null
            else -> true
        }
    }
    return rows.sortedWith { a, b ->
        val at = a.optJSONObject("latest")?.optLong("created")
        val bt = b.optJSONObject("latest")?.optLong("created")
        val order = when {
            sort == "name" -> 0
            at == null && bt == null -> 0
            at == null -> 1
            bt == null -> -1
            sort == "oldest" -> at.compareTo(bt)
            else -> bt.compareTo(at)
        }
        if (order != 0) order else a.getString("id").compareTo(b.getString("id"), ignoreCase = true)
    }
}

@Composable
private fun ReportChoice(label: String, value: String, choices: List<Pair<String, String>>, enabled: Boolean = true, select: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        LelloFilterChip(selected = true, enabled = enabled, onClick = { if (enabled) expanded = true }, label = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("$label: ${choices.first { it.first == value }.second}", style = MaterialTheme.typography.labelLarge)
                Icon(Icons.Default.ArrowDropDown, null, Modifier.size(18.dp))
            }
        })
        DropdownMenu(expanded, { expanded = false }) {
            choices.forEach { (key, title) -> DropdownMenuItem(text = { Text(title) }, onClick = { expanded = false; select(key) }) }
        }
    }
}

@Composable
private fun CompactReportRow(title: String, subtitle: String, status: String, enabled: Boolean, open: () -> Unit) {
    val palette = LocalLelloPalette.current
    val tone = when (status) { "failed" -> "error"; "partial" -> "warning"; "complete" -> "success"; else -> "info" }
    Column {
        Row(Modifier.fillMaxWidth().clickable(enabled = enabled, role = Role.Button, onClickLabel = "Open $title", onClick = open)
            .heightIn(min = 64.dp).padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(subtitle, style = MaterialTheme.typography.bodySmall, color = palette["on-$tone-container"], maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

@Composable
internal fun ReportRunDetails(run: JSONObject) {
    RunStatus(run.getString("status"))
    if (reportActive(run.getString("status"))) Muted("Progress refreshes automatically while this screen is open. You can leave and return later.")
    if (!run.isNull("error")) LelloAlert(run.getString("error"), tone = LelloTone.Error)
    run.optJSONObject("content")?.let { content ->
        LelloSection(content.getString("subject")) {
            SelectionContainer { Text(content.getString("summary")) }
            val sections = content.getJSONArray("sections")
            for (i in 0 until sections.length()) {
                val section = sections.getJSONObject(i)
                LelloCard(Modifier.fillMaxWidth()) {
                    Text(section.getString("title"), style = MaterialTheme.typography.titleMedium)
                    SelectionContainer { Text(section.getString("text")) }
                }
            }
        }
    }
    LelloSection("Checks") {
        val steps = run.getJSONArray("steps")
        val completed = (0 until steps.length()).count { steps.getJSONObject(it).getString("status") !in listOf("pending", "running", "not_run") }
        Muted("$completed of ${steps.length()} checks finished")
        for (i in 0 until steps.length()) {
            val step = steps.getJSONObject(i)
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(step.getString("id"), style = MaterialTheme.typography.titleMedium)
                Muted(step.getString("status").replace("_", " ").replaceFirstChar { it.uppercase() })
                if (!step.isNull("error")) Text(step.getString("error"), color = MaterialTheme.colorScheme.error)
            }
        }
    }
    val deliveries = run.optJSONArray("deliveries")
    if (deliveries != null && deliveries.length() > 0) LelloSection("Delivery") {
        for (i in 0 until deliveries.length()) {
            val delivery = deliveries.getJSONObject(i)
            Text("${delivery.getString("destination")} · ${delivery.getString("status")}")
            if (!delivery.isNull("error")) Muted(delivery.getString("error"))
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Muted("Start · ${reportTime(run.getLong("period_start"))}")
        Muted("End · ${reportTime(run.getLong("created"))}")
        Muted(if (run.optBoolean("send")) "Delivery requested by the original run" else "Saved in Talìa · No messages sent")
        SelectionContainer { Muted("Run ID · ${run.getString("id")}") }
    }
}
