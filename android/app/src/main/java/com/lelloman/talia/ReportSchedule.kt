package com.lelloman.talia

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.lelloman.lellodesign.*
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

internal fun scheduleSummary(value: JSONObject): String {
    val schedule = value.optJSONObject("schedule") ?: return "No schedule"
    val timing = if (schedule.optString("kind") == "daily") {
        val days = schedule.optJSONArray("weekdays")
        val names = listOf("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
        val label = if (days == null || days.length() == 0) "Every day" else
            (0 until days.length()).joinToString(", ") { names.getOrElse(days.optInt(it) - 1) { "?" } }
        "$label at ${schedule.optString("time")} · ${schedule.optString("zone")}"
    } else "Every ${schedule.optLong("every_ms").toBigDecimal().divide(1000.toBigDecimal()).stripTrailingZeros().toPlainString()} seconds"
    return if (value.optBoolean("enabled")) timing else "Paused · $timing"
}

internal fun scheduleDraft(kind: String, time: String, zone: String, seconds: String, days: Set<Int>): JSONObject? {
    if (kind == "none") return null
    if (kind == "interval") {
        val amount = seconds.toBigDecimalOrNull() ?: error("Enter an interval in seconds.")
        require(amount >= 60.toBigDecimal() && amount <= 31_536_000.toBigDecimal()) { "Interval must be between 60 seconds and 365 days." }
        return JSONObject().put("kind", kind).put("every_ms", try { amount.multiply(1000.toBigDecimal()).longValueExact() } catch (_: ArithmeticException) { error("Use at most millisecond precision.") })
    }
    require(Regex("([01][0-9]|2[0-3]):[0-5][0-9]").matches(time)) { "Use a time in HH:mm format." }
    require(zone in ZoneId.getAvailableZoneIds()) { "Choose an IANA timezone, for example Europe/Rome." }
    return JSONObject().put("kind", "daily").put("time", time).put("zone", zone)
        .put("weekdays", JSONArray(days.sorted()))
}

@Composable
internal fun ReportSchedulePanel(connection: NativeConnection) {
    val value = connection.reportSchedule
    var editing by rememberSaveable(connection.reportSelected) { mutableStateOf<String?>(null) }
    connection.scheduleMessage?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
    if (connection.pendingSchedule) {
        LelloAlert("A schedule save needs confirmation", tone = LelloTone.Warning)
        LelloOutlinedButton(connection::retryReportSchedule, enabled = !connection.reportsBusy) { Text("Check schedule save") }
    }
    if (value == null) return
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Schedule", style = MaterialTheme.typography.titleMedium)
        Text(scheduleSummary(value), style = MaterialTheme.typography.bodyMedium)
        if (!value.isNull("next_due")) {
            val zone = value.optJSONObject("schedule")?.optString("zone")?.takeIf { it.isNotBlank() } ?: "UTC"
            val next = DateTimeFormatter.ofPattern("EEE dd MMM yyyy HH:mm z")
                .format(Instant.ofEpochMilli(value.getLong("next_due")).atZone(ZoneId.of(zone)))
            Text("Next run: $next", style = MaterialTheme.typography.bodySmall)
        } else Text("No scheduled run", style = MaterialTheme.typography.bodySmall)
        LelloTextButton({ editing = if (editing == null) value.toString() else null }, enabled = !connection.reportsBusy && !connection.pendingSchedule) {
            Text(if (editing != null) "Cancel schedule edit" else "Edit schedule")
        }
        editing?.let { snapshot ->
            val original = JSONObject(snapshot)
            ScheduleEditor(original, !connection.reportsBusy && !connection.pendingSchedule) { enabled, schedule ->
                connection.saveReportSchedule(enabled, schedule, original.getLong("version")); editing = null
            }
        }
    }
}

@Composable
private fun ScheduleEditor(value: JSONObject, editable: Boolean, save: (Boolean, JSONObject?) -> Unit) {
    val initial = value.optJSONObject("schedule")
    var enabled by rememberSaveable { mutableStateOf(value.optBoolean("enabled")) }
    var kind by rememberSaveable { mutableStateOf(initial?.optString("kind") ?: "none") }
    var time by rememberSaveable { mutableStateOf(initial?.optString("time", "09:00") ?: "09:00") }
    var zone by rememberSaveable { mutableStateOf(initial?.optString("zone", "Europe/Rome") ?: "Europe/Rome") }
    var seconds by rememberSaveable { mutableStateOf((initial?.optLong("every_ms", 3600000) ?: 3600000).toBigDecimal().divide(1000.toBigDecimal()).stripTrailingZeros().toPlainString()) }
    var weekdays by rememberSaveable { mutableStateOf<List<Int>>(initial?.optJSONArray("weekdays")?.let { a -> (0 until a.length()).map { a.getInt(it) } } ?: emptyList()) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    LelloCard(Modifier.fillMaxWidth()) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Checkbox(enabled, { enabled = it }, modifier = Modifier.semantics { contentDescription = "Schedule enabled" }, enabled = editable && kind != "none")
            Text("Schedule enabled", Modifier.padding(top = 12.dp))
        }
        LelloSortMenu(listOf(LelloListOption("none", "None"), LelloListOption("daily", "Daily"), LelloListOption("interval", "Interval")), kind,
            { kind = it; if (kind == "none") enabled = false }, enabled = editable, label = "Repeat")
        if (kind == "daily") {
            LelloTextField(time, { time = it }, { Text("Time (HH:mm)") }, enabled = editable, modifier = Modifier.fillMaxWidth())
            LelloTextField(zone, { zone = it }, { Text("Timezone (IANA)") }, enabled = editable, modifier = Modifier.fillMaxWidth())
            Text("Weekdays · none selected means every day", style = MaterialTheme.typography.bodySmall)
            listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday").forEachIndexed { index, label ->
                Row {
                    Checkbox(index + 1 in weekdays, { checked -> weekdays = if (checked) weekdays + (index + 1) else weekdays - (index + 1) }, modifier = Modifier.semantics { contentDescription = label }, enabled = editable)
                    Text(label, Modifier.padding(top = 12.dp))
                }
            }
        } else if (kind == "interval") {
            LelloTextField(seconds, { seconds = it }, { Text("Interval in seconds") }, enabled = editable, modifier = Modifier.fillMaxWidth())
            Text("Elapsed interval; first run is one interval after enabling or changing it.", style = MaterialTheme.typography.bodySmall)
        }
        val destinations = value.optJSONArray("destinations")
        Text(if (destinations == null || destinations.length() == 0) "No delivery destinations configured. Enabling a schedule requires one."
            else "Scheduled runs send results to: " + (0 until destinations.length()).joinToString { destinations.getString(it) }, style = MaterialTheme.typography.bodySmall)
        error?.let { LelloAlert(it, tone = LelloTone.Error) }
        LelloButton({
            try { val schedule = scheduleDraft(kind, time.trim(), zone.trim(), seconds.trim(), weekdays.toSet()); save(enabled && schedule != null, schedule) }
            catch (e: IllegalArgumentException) { error = e.message }
            catch (e: IllegalStateException) { error = e.message }
        }, enabled = editable) { Text("Save schedule") }
        Text("Pausing affects future runs. Runs already started continue.", style = MaterialTheme.typography.bodySmall)
    }
}
