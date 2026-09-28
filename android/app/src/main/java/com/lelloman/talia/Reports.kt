package com.lelloman.talia

import androidx.activity.compose.BackHandler
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
            LelloCard(Modifier.fillMaxWidth()) {
                Text("Run a fresh report", style = MaterialTheme.typography.titleMedium)
                Muted("Results are saved here. This run won’t send Telegram or email messages.")
                LelloButton(connection::runReport, enabled = !connection.reportsBusy && connection.pendingReport == null && definition?.optBoolean("available", true) != false) { Text("Run report") }
            }
            LelloSection("Previous runs") {
                if (connection.reportHistory.length() == 0 && !connection.reportsBusy && connection.reportsMessage == null)
                    LelloState("No runs yet", "Run this report to create its first result.", modifier = Modifier.fillMaxWidth())
                for (i in 0 until connection.reportHistory.length()) {
                    val run = connection.reportHistory.getJSONObject(i)
                    LelloCard(Modifier.fillMaxWidth()) {
                        RunStatus(run.getString("status"))
                        Muted(reportTime(run.getLong("created")))
                        LelloTextButton({ connection.selectReportRun(run.getString("id")) }, enabled = !connection.reportsBusy) { Text("View run") }
                    }
                }
                if (connection.reportsNext != null) LelloOutlinedButton(connection::moreReportRuns, enabled = !connection.reportsBusy) { Text("Load older runs") }
            }
        }
        else -> {
            val definitions = connection.reportDefinitions
            if (definitions?.length() == 0) LelloState("No reports configured", "Reports created on the server will appear here.", modifier = Modifier.fillMaxWidth())
            for (i in 0 until (definitions?.length() ?: 0)) {
                val report = definitions!!.getJSONObject(i)
                LelloCard(Modifier.fillMaxWidth()) {
                    Text(report.getString("id"), style = MaterialTheme.typography.titleLarge)
                    Muted("${report.getInt("steps")} checks · Covers the previous ${report.getLong("period_ms") / 3_600_000.0} hours")
                    Muted(if (report.optBoolean("enabled") && report.optBoolean("scheduled")) "Scheduled on the server" else "Automatic schedule is off")
                    if (!report.optBoolean("available")) LelloAlert("This report needs an update before it can run.", tone = LelloTone.Warning)
                    report.optJSONObject("latest")?.let { latest -> RunStatus(latest.getString("status")); Muted("Last run · ${reportTime(latest.getLong("created"))}") }
                        ?: Muted("Not run yet")
                    LelloOutlinedButton({ connection.selectReport(report.getString("id")) }, enabled = !connection.reportsBusy) { Text("Open report") }
                }
            }
        }
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
