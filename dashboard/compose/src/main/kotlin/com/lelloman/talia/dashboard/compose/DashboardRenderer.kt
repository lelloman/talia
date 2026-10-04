package com.lelloman.talia.dashboard.compose

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.lelloman.lellodesign.LelloDimensions
import com.lelloman.lellodesign.LelloOutlinedButton
import com.lelloman.lellodesign.LelloSlider
import com.lelloman.lellodesign.LocalLelloPalette
import org.json.JSONObject

/** Renderer callback: the ViewModel action named by a node, the node ID and the event value. */
typealias DashboardEvent = (action: String, target: String, value: Any?) -> Unit

private val LocalInCard = staticCompositionLocalOf { false }

/** Renders a resolved contract v1 tree with LelloDesign. Unknown node types are rejected by the shared validator. */
@Composable
fun DashboardContent(tree: JSONObject, onEvent: DashboardEvent, modifier: Modifier = Modifier) {
    Box(modifier) { Node(tree, onEvent, Modifier.fillMaxWidth()) }
}

private fun JSONObject.props(): JSONObject = optJSONObject("props") ?: JSONObject()
private fun JSONObject.kids(): List<JSONObject> = optJSONArray("children")?.let { a -> (0 until a.length()).map { a.getJSONObject(it) } } ?: emptyList()
private fun JSONObject.length(key: String): Dp? = optString(key).takeIf { it.endsWith("dp") || it.endsWith("px") }?.dropLast(2)?.toFloatOrNull()?.dp
private fun JSONObject.action(key: String): String? = optJSONObject(key)?.optString("action")?.ifEmpty { null }

@Composable
private fun tone(name: String): Color {
    val palette = LocalLelloPalette.current
    return when (name) {
        "muted" -> palette["text-secondary"]
        "success" -> palette["success"]
        "warning" -> palette["warning"]
        "error" -> palette["error"]
        else -> palette["text"]
    }
}

/** Layout children stretch like the web flex column; compact controls keep their intrinsic width. */
private fun stretches(node: JSONObject) = node.optString("type") !in setOf("Button", "Status", "SegmentedControl")

@Composable
private fun Node(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier = Modifier, role: TextRole = TextRole.Plain) {
    val p = node.props()
    if (p.optString("visibility") == "collapsed") return
    var m = modifier
    if (p.optString("visibility") == "hidden") m = m.alpha(0f)
    when (p.optString("width")) { "fill" -> m = m.fillMaxWidth(); "auto", "" -> Unit; else -> p.length("width")?.let { m = m.width(it) } }
    when (p.optString("height")) { "fill" -> m = m.fillMaxHeight(); "auto", "" -> Unit; else -> if (node.optString("type") != "Chart") p.length("height")?.let { m = m.height(it) } }
    when (node.optString("type")) {
        "Column", "Scroll" -> Container(node, onEvent, m)
        "Row" -> RowNode(node, onEvent, m)
        "Grid" -> GridNode(node, onEvent, m)
        "Text" -> TextNode(node, m, role)
        "Status" -> StatusNode(node, m)
        "Chart" -> DashboardChart(p, m)
        "Meter" -> MeterNode(p, m)
        "Button" -> ButtonNode(node, onEvent, m)
        "SegmentedControl" -> Segmented(node, onEvent, m)
        "Slider" -> SliderNode(node, onEvent, m)
        "Switch" -> SwitchNode(node, onEvent, m)
    }
}

private enum class TextRole { Plain, CardTitle, RowLead, RowValue }

@Composable
private fun Surface(node: JSONObject, modifier: Modifier, content: @Composable (Modifier) -> Unit) {
    val p = node.props()
    val inner = Modifier.padding(p.length("padding") ?: 0.dp)
    if (p.optString("surface") == "card") {
        val palette = LocalLelloPalette.current
        Surface(modifier, shape = RoundedCornerShape(LelloDimensions.radiusPanel), color = palette["surface"],
            border = BorderStroke(1.dp, palette["border-subtle"])) {
            CompositionLocalProvider(LocalInCard provides true) { content(inner) }
        }
    } else content(modifier.then(inner))
}

@Composable
private fun Container(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val card = node.props().optString("surface") == "card"
    Surface(node, modifier) { inner ->
        Column(inner, verticalArrangement = Arrangement.spacedBy(node.props().length("gap") ?: 0.dp)) {
            node.kids().forEachIndexed { index, child ->
                val title = card && index == 0 && child.optString("type") == "Text" && child.props().optString("variant", "body") == "body"
                Node(child, onEvent, if (stretches(child)) Modifier.fillMaxWidth() else Modifier,
                    if (title) TextRole.CardTitle else TextRole.Plain)
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun RowNode(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val inCard = LocalInCard.current
    val gap = node.props().length("gap") ?: 0.dp
    Surface(node, modifier) { inner ->
        FlowRow(inner, horizontalArrangement = if (inCard) Arrangement.SpaceBetween else Arrangement.spacedBy(gap),
            verticalArrangement = Arrangement.spacedBy(2.dp), itemVerticalAlignment = Alignment.CenterVertically) {
            var texts = 0
            node.kids().forEach { child ->
                val caption = child.props().optString("variant") == "caption"
                val role = if (inCard && child.optString("type") == "Text" && !caption) { texts++; if (texts == 1) TextRole.RowLead else TextRole.RowValue } else TextRole.Plain
                Node(child, onEvent, Modifier, role)
            }
        }
    }
}

@Composable
private fun GridNode(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val p = node.props()
    val columns = p.optInt("columns", 1).coerceAtLeast(1)
    val gap = p.length("gap") ?: 0.dp
    val children = node.kids().filter { it.props().optString("visibility") != "collapsed" }
    Surface(node, modifier) { inner ->
        Column(inner, verticalArrangement = Arrangement.spacedBy(gap)) {
            children.chunked(columns).forEach { row ->
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
                    row.forEach { Node(it, onEvent, Modifier.weight(1f).fillMaxHeight()) }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

@Composable
private fun TextNode(node: JSONObject, modifier: Modifier, role: TextRole) {
    val p = node.props()
    val text = p.opt("text")?.toString().orEmpty()
    if (text.isEmpty()) return
    val variant = p.optString("variant", "body")
    val toneName = p.optString("tone", "neutral")
    var style = when (variant) {
        "heading" -> MaterialTheme.typography.headlineMedium.copy(fontWeight = FontWeight.Bold)
        "metric" -> if (toneName == "muted") TextStyle(fontSize = 28.sp, lineHeight = 44.sp, fontWeight = FontWeight.Bold)
            else TextStyle(fontSize = 38.sp, lineHeight = 44.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.5).sp, fontFeatureSettings = "tnum")
        "caption" -> MaterialTheme.typography.bodySmall.copy(fontSize = 13.sp, lineHeight = 18.sp)
        else -> MaterialTheme.typography.bodyLarge
    }
    var color = tone(toneName)
    when (role) {
        TextRole.CardTitle -> { style = MaterialTheme.typography.titleSmall; color = tone("muted") }
        TextRole.RowLead -> style = style.copy(fontWeight = FontWeight.SemiBold)
        TextRole.RowValue -> style = style.copy(fontWeight = FontWeight.SemiBold, fontFeatureSettings = "tnum")
        TextRole.Plain -> Unit
    }
    Text(text, modifier.semantics {
        if (variant == "heading") heading()
        p.optString("label").ifEmpty { null }?.let { contentDescription = it }
    }, color = color, style = style)
}

@Composable
private fun StatusNode(node: JSONObject, modifier: Modifier) {
    val p = node.props()
    val text = p.opt("text")?.toString().orEmpty()
    if (text.isEmpty()) return
    val toneName = p.optString("tone", "neutral")
    val live = Modifier.semantics { liveRegion = LiveRegionMode.Polite }
    if (LocalInCard.current) {
        Text(text, modifier.then(live), color = tone(toneName),
            style = if (toneName == "muted") MaterialTheme.typography.bodySmall.copy(fontSize = 13.sp) else MaterialTheme.typography.titleMedium)
        return
    }
    // Standalone status reads as a badge, matching the web dashboard.
    val palette = LocalLelloPalette.current
    val (container, border, onColor, dot) = when (toneName) {
        "success", "warning", "error" -> listOf(palette["$toneName-container"], palette["$toneName-border"], palette["on-$toneName-container"], palette[toneName])
        else -> listOf(palette["surface-sunken"], palette["border-subtle"], palette["text-secondary"], palette["text-secondary"])
    }
    Row(modifier.then(live).clip(CircleShape).background(container).border(1.dp, border, CircleShape).padding(start = 10.dp, end = 12.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
        Text(text, color = onColor, style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.SemiBold))
    }
}

@Composable
private fun MeterNode(p: JSONObject, modifier: Modifier) {
    val palette = LocalLelloPalette.current
    val min = p.optDouble("min", 0.0); val max = p.optDouble("max", 100.0)
    val unavailable = p.has("unavailable")
    val fraction = if (unavailable || max <= min) 0f else ((p.optDouble("value", min) - min) / (max - min)).toFloat().coerceIn(0f, 1f)
    val fill = when (p.optString("tone")) {
        "warning" -> palette["warning"]; "error" -> palette["error"]; "success" -> palette["success"]
        "muted" -> palette["text-muted"]; else -> palette["primary"]
    }
    Box(modifier.fillMaxWidth().height(8.dp).clip(CircleShape).background(palette["surface-sunken"]).border(1.dp, palette["border-subtle"], CircleShape)
        .semantics {
            contentDescription = p.optString("label")
            if (!unavailable) progressBarRangeInfo = ProgressBarRangeInfo(p.optDouble("value", min).toFloat(), min.toFloat()..max.toFloat())
            else stateDescription = "Unavailable"
        }) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(fraction).clip(CircleShape).background(fill))
    }
}

@Composable
private fun ButtonNode(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val p = node.props()
    val action = p.action("onClick")
    LelloOutlinedButton(onClick = { action?.let { onEvent(it, node.getString("id"), null) } }, modifier = modifier.semantics {
        p.optString("label").ifEmpty { null }?.let { contentDescription = it }
    }, enabled = p.optBoolean("enabled", true) && action != null) { Text(p.optString("text")) }
}

@Composable
private fun Segmented(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val palette = LocalLelloPalette.current
    val label = node.props().optString("label")
    Row(modifier.height(40.dp).clip(CircleShape).border(1.dp, palette["border-control"], CircleShape)
        .semantics { contentDescription = label }.selectableGroup()) {
        node.kids().forEachIndexed { index, option ->
            val p = option.props()
            val selected = p.optBoolean("selected")
            val action = p.action("onClick")
            if (index > 0) Box(Modifier.fillMaxHeight().width(1.dp).background(palette["border-control"]))
            Row(Modifier.fillMaxHeight().background(if (selected) palette["primary-container"] else Color.Transparent)
                .selectable(selected = selected, enabled = p.optBoolean("enabled", true) && action != null, role = Role.RadioButton) {
                    if (!selected) action?.let { onEvent(it, option.getString("id"), null) }
                }
                .padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                val color = if (selected) palette["on-primary-container"] else palette["text"]
                if (selected) Text("✓", Modifier.clearAndSetSemantics {}, color = color, style = MaterialTheme.typography.labelLarge)
                // The selectable merges this label, so TalkBack names and activates the same node.
                Text(p.optString("text"), Modifier.semantics { p.optString("label").ifEmpty { null }?.let { contentDescription = it } },
                    color = color, style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.SemiBold))
            }
        }
    }
}

@Composable
private fun SliderNode(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val p = node.props()
    val action = p.action("onChange")
    val min = p.optDouble("min").toFloat(); val max = p.optDouble("max").toFloat()
    val step = p.optDouble("step", 1.0).toFloat()
    var value by remember(p.optDouble("value")) { mutableFloatStateOf(p.optDouble("value").toFloat()) }
    LelloSlider(value = value, onValueChange = { value = (min + Math.round((it - min) / step) * step).coerceIn(min, max) },
        label = p.optString("label"), modifier = modifier, enabled = p.optBoolean("enabled", true) && action != null,
        valueRange = min..max, onValueChangeFinished = { action?.let { onEvent(it, node.getString("id"), value.toDouble()) } })
}

@Composable
private fun SwitchNode(node: JSONObject, onEvent: DashboardEvent, modifier: Modifier) {
    val p = node.props()
    val action = p.action("onChange")
    val enabled = p.optBoolean("enabled", true) && action != null
    Row(modifier.clickable(enabled = enabled) { action?.let { onEvent(it, node.getString("id"), !p.optBoolean("value")) } }
        .semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
        Text(p.optString("label"), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = p.optBoolean("value"), onCheckedChange = null, enabled = enabled)
    }
}
