package com.lelloman.talia

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.material.icons.Icons
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ReportsScreen(connection: NativeConnection, modifier: Modifier = Modifier, setup: () -> Unit) {
    val refresh = { if (connection.signedIn && !connection.reportsBusy) connection.refreshReports() }
    val content: @Composable () -> Unit = {
        LelloWorkspace(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).semantics {
            if (connection.signedIn) customActions = listOf(CustomAccessibilityAction("Refresh reports") {
                refresh(); true
            })
        }) { Reports(connection, setup) }
    }
    if (connection.signedIn) {
        PullToRefreshBox(isRefreshing = connection.reportsBusy, onRefresh = refresh, modifier = modifier) { content() }
    } else {
        Box(modifier) { content() }
    }
}

@Composable
internal fun Reports(connection: NativeConnection, setup: () -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var sort by rememberSaveable { mutableStateOf("newest") }
    var filter by rememberSaveable { mutableStateOf("all") }
    var schedule by rememberSaveable { mutableStateOf("all") }
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
                ReportSchedulePanel(connection)
                LelloButton(connection::runReport, enabled = !connection.reportsBusy && connection.pendingReport == null && definition?.optBoolean("available", true) != false) { Text("Run report") }
                Muted("Saved in Talìa · No Telegram or email sent")
                LelloListControls(
                    sortOptions = reportSortOptions,
                    selectedSort = connection.reportSort,
                    onSortSelected = { connection.filterReportHistory(it, connection.reportFilter) },
                    filterGroups = listOf(historyFilterGroup),
                    selectedFilters = if (connection.reportFilter == "all") emptySet() else setOf(connection.reportFilter),
                    onFiltersApplied = { connection.filterReportHistory(connection.reportSort, it.firstOrNull() ?: "all") },
                    enabled = !connection.reportsBusy,
                )
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
            val reports = catalogReports(definitions, query, sort, filter, schedule)
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                LelloTextField(query, { query = it }, { Text("Search reports") }, modifier = Modifier.fillMaxWidth(),
                    suffix = { if (query.isNotEmpty()) LelloTextButton({ query = "" }) { Text("Clear") } })
                LelloListControls(
                    sortOptions = reportSortOptions + LelloListOption("name", "Name A–Z"),
                    selectedSort = sort, onSortSelected = { sort = it },
                    filterGroups = catalogFilterGroups,
                    selectedFilters = buildSet {
                        if (filter != "all") add("status:$filter")
                        if (schedule != "all") add("schedule:$schedule")
                    },
                    onFiltersApplied = { criteria ->
                        filter = criteria.firstOrNull { it.startsWith("status:") }?.removePrefix("status:") ?: "all"
                        schedule = criteria.firstOrNull { it.startsWith("schedule:") }?.removePrefix("schedule:") ?: "all"
                    },
                    enabled = !connection.reportsBusy,
                )
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

private val reportSortOptions = listOf(LelloListOption("newest", "Newest first"), LelloListOption("oldest", "Oldest first"))
private val historyFilterGroup = LelloFilterGroup("status", "Status", listOf(
    LelloListOption("active", "Running"), LelloListOption("complete", "Completed"),
    LelloListOption("issues", "With issues"), LelloListOption("failed", "Failed")), anyLabel = "All statuses")
private val catalogFilterGroups = listOf(
    LelloFilterGroup("status", "Status", listOf(LelloListOption("status:issues", "Needs attention"),
        LelloListOption("status:active", "Running"), LelloListOption("status:never", "Never run")), anyLabel = "All statuses"),
    LelloFilterGroup("schedule", "Schedule", listOf(LelloListOption("schedule:scheduled", "Scheduled"),
        LelloListOption("schedule:off", "Schedule off")), anyLabel = "Any schedule"),
)
private fun shortReportTime(ms: Long) = DateTimeFormatter.ofPattern("dd MMM · HH:mm").format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))
private fun compactStatus(status: String) = when (status) {
    "complete" -> "Completed"; "partial" -> "With issues"; "failed" -> "Failed"
    "queued" -> "Queued"; "running" -> "Running"; "delivering" -> "Sending"; "never" -> "Never run"; else -> status
}
internal fun catalogReports(data: org.json.JSONArray?, query: String, sort: String, filter: String, schedule: String = "all"): List<JSONObject> {
    val rows = (0 until (data?.length() ?: 0)).map { data!!.getJSONObject(it) }.filter { report ->
        val status = report.optJSONObject("latest")?.optString("status")
        val scheduled = report.optBoolean("enabled") && report.optBoolean("scheduled")
        val matchesSchedule = when (schedule) { "scheduled" -> scheduled; "off" -> !scheduled; else -> true }
        matchesSchedule && report.getString("id").contains(query.trim(), ignoreCase = true) && when (filter) {
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
    when (run.optString("severity")) {
        "warning" -> LelloAlert("Report findings: warning", tone = LelloTone.Warning)
        "error" -> LelloAlert("Report findings: error", tone = LelloTone.Error)
        "nominal" -> Muted("Report findings: nominal")
        "unknown" -> Muted("Report severity unavailable")
    }
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
