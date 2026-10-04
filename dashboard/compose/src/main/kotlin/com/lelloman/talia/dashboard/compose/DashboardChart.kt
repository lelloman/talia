package com.lelloman.talia.dashboard.compose

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.lelloman.lellodesign.LocalLelloPalette
import org.json.JSONObject

/** Contract v1 Chart: same geometry and semantics as the web canvas painter. */
@Composable
internal fun DashboardChart(p: JSONObject, modifier: Modifier) {
    val palette = LocalLelloPalette.current
    val measurer = rememberTextMeasurer()
    val array = p.optJSONArray("values")
    val values = remember(array.toString()) { (0 until (array?.length() ?: 0)).map { i -> array!!.opt(i).let { (it as? Number)?.toDouble()?.takeIf(Double::isFinite) } } }
    val finite = values.filterNotNull()
    val fixed = p.has("min") && p.has("max")
    val min = if (fixed) p.getDouble("min") else minOf(0.0, finite.minOrNull() ?: 0.0)
    val max = if (fixed) p.getDouble("max") else maxOf(1.0, finite.maxOrNull() ?: 1.0)
    val unit = p.optString("unit")
    val threshold = p.optDouble("threshold").takeIf { p.has("threshold") && it in min..max }
    val label = p.optString("label")
    val labelStyle = TextStyle(fontSize = 11.sp, color = palette["text-secondary"])
    val primary = palette["primary"]; val warning = palette["warning"]; val grid = palette["border-subtle"]; val surface = palette["surface"]
    val description = buildString {
        append(label)
        if (fixed) append("; range ${fmt(min)} to ${fmt(max)}$unit")
        threshold?.let { append("; high reference ${fmt(it)}$unit") }
        if (p.has("startLabel") && p.has("endLabel")) append("; from ${p.optString("startLabel")} to ${p.optString("endLabel")}")
        append(": "); append(if (finite.isEmpty()) "No samples" else finite.joinToString { fmt(it) })
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Canvas(Modifier.fillMaxWidth().height(p.optString("height").removeSuffix("dp").toFloatOrNull()?.dp ?: 120.dp).semantics { contentDescription = description }) {
            val axis = { v: Double -> (if (v == Math.rint(v)) v.toLong().toString() else "%.1f".format(v)) + unit }
            val x0 = if (fixed) maxOf(37.dp.toPx(), measurer.measure(axis(max), labelStyle).size.width + 8.dp.toPx()) else 4.dp.toPx()
            val x1 = size.width - 7.dp.toPx(); val y0 = 9.dp.toPx(); val y1 = size.height - (if (fixed) 25.dp else 5.dp).toPx()
            if (x1 <= x0 || y1 <= y0) return@Canvas
            val span = max - min
            fun y(v: Double) = (y1 - ((v.coerceIn(min, max) - min) / span * (y1 - y0))).toFloat()
            fun x(i: Int) = x0 + i * (x1 - x0) / maxOf(1, values.size - 1)
            if (fixed) {
                for (i in 0..4) {
                    val v = min + span * i / 4; val at = y(v)
                    drawLine(grid, Offset(x0, at), Offset(x1, at), 1.dp.toPx(), pathEffect = if (i > 0) PathEffect.dashPathEffect(floatArrayOf(2.dp.toPx(), 3.dp.toPx())) else null)
                    val text = measurer.measure(axis(v), labelStyle)
                    drawText(text, topLeft = Offset(x0 - 6.dp.toPx() - text.size.width, at - text.size.height / 2f))
                }
                p.optString("startLabel").ifEmpty { null }?.let { val t = measurer.measure(it, labelStyle); drawText(t, topLeft = Offset(x0, size.height - t.size.height)) }
                p.optString("endLabel").ifEmpty { null }?.let { val t = measurer.measure(it, labelStyle); drawText(t, topLeft = Offset(x1 - t.size.width, size.height - t.size.height)) }
            }
            // Contiguous runs; missing samples remain visible gaps.
            val runs = mutableListOf<MutableList<Offset>>(); var run: MutableList<Offset>? = null
            values.forEachIndexed { i, v -> if (v == null) run = null else { if (run == null) run = mutableListOf<Offset>().also(runs::add); run!!.add(Offset(x(i), y(v))) } }
            val line = Path().apply { runs.forEach { r -> r.forEachIndexed { i, o -> if (i == 0) moveTo(o.x, o.y) else lineTo(o.x, o.y) } } }
            val fill = Brush.verticalGradient(listOf(primary.copy(alpha = 0.28f), primary.copy(alpha = 0f)), startY = y0, endY = y1)
            runs.filter { it.size > 1 }.forEach { r ->
                drawPath(Path().apply { moveTo(r.first().x, y1); r.forEach { lineTo(it.x, it.y) }; lineTo(r.last().x, y1); close() }, fill)
            }
            val stroke = Stroke(1.75.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round)
            drawPath(line, primary, style = stroke)
            threshold?.let { t ->
                val at = y(t)
                clipRect(0f, 0f, size.width, at) { drawPath(line, warning, style = stroke) }
                drawLine(warning, Offset(x0, at), Offset(x1, at), 1.25.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(5.dp.toPx(), 4.dp.toPx())))
                val text = measurer.measure("${fmt(t)}$unit high", labelStyle.copy(color = warning))
                drawText(text, topLeft = Offset(x1 - 4.dp.toPx() - text.size.width, maxOf(0f, at - 4.dp.toPx() - text.size.height)))
            }
            runs.lastOrNull()?.lastOrNull()?.takeIf { it.x >= x1 - 1f }?.let { end ->
                val color = if (threshold != null && (finite.lastOrNull() ?: 0.0) > threshold) warning else primary
                drawCircle(surface, 5.5.dp.toPx(), end); drawCircle(color, 3.5.dp.toPx(), end)
            }
        }
        Text(label + if (finite.isEmpty()) " · No samples" else "", style = MaterialTheme.typography.bodySmall, color = palette["text-secondary"])
    }
}

private fun fmt(v: Double) = if (v == Math.rint(v)) v.toLong().toString() else "%.1f".format(v)

