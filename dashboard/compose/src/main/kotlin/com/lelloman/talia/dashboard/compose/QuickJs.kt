package com.lelloman.talia.dashboard.compose

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.concurrent.Executors

/** Bounded QuickJS contexts from `dashboard/native`. Contexts are thread-local, so all calls share one thread. */
internal object QuickJs {
    init { System.loadLibrary("talia_dashboard_runtime") }
    val thread: CoroutineDispatcher = Executors.newSingleThreadExecutor { Thread(it, "talia-dashboard-js") }.asCoroutineDispatcher()

    @JvmStatic private external fun evaluate(source: String, ui: Boolean, reset: Boolean): String
    @JvmStatic private external fun retire()

    class GuestFailure(message: String) : Exception(message)

    /** Evaluates [source] (ending in a string expression); returns the string and any bridge requests. */
    suspend fun eval(source: String, ui: Boolean, reset: Boolean = false): Pair<String, List<JSONObject>> = withContext(thread) {
        val reply = JSONObject(evaluate(source, ui, reset))
        if (reply.has("error")) throw GuestFailure(reply.getString("error"))
        val out = reply.optJSONArray("out")
        reply.getString("value") to (0 until (out?.length() ?: 0)).map { out!!.getJSONObject(it) }
    }

    suspend fun stop() = withContext(thread) { retire() }
}
