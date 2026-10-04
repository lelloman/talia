package com.lelloman.talia.dashboard.compose

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONArray
import org.json.JSONObject
import java.security.SecureRandom
import java.util.UUID

/** Authenticated access to a Talìa server. The app owns credentials, routing and session expiry. */
interface DashboardTransport {
    /** Dashboard catalog and default selection (`/native/dashboards`). */
    suspend fun dashboards(body: JSONObject): JSONObject
    /** Client registration and delivery (`/native/clients`); [installation] travels outside the guest. */
    suspend fun clients(body: JSONObject, installation: String): JSONObject
    /** Engine data envelope (`/native/engine`). */
    suspend fun engine(body: JSONObject): JSONObject
}

class DashboardServerError(val code: String) : Exception(code)

private fun secret(): String = ByteArray(32).also(SecureRandom()::nextBytes).joinToString("") { "%02x".format(it) }

/** Host-only installation identity. Credentials never enter a QuickJS context. */
internal class ClientRegistry(context: Context, private val transport: DashboardTransport) {
    private val prefs = context.getSharedPreferences("talia_dashboard_client", Context.MODE_PRIVATE)
    private val credential = prefs.getString("credential", null) ?: secret()
    private val slot = prefs.getString("slot", null) ?: UUID.randomUUID().toString()
    private val owner = prefs.getString("owner", null) ?: secret()
    init { prefs.edit().putString("credential", credential).putString("slot", slot).putString("owner", owner).apply() }

    private suspend fun send(body: JSONObject): JSONObject {
        val reply = transport.clients(body, credential)
        if (reply.has("error")) throw DashboardServerError(reply.getString("error"))
        return reply.optJSONObject("value") ?: JSONObject()
    }
    private fun slotRequest(op: String) = JSONObject().put("op", op).put("slot", slot).put("owner", owner)
    private suspend fun enroll() { send(JSONObject().put("op", "register").put("name", "Talìa Android").put("platform", "android")) }

    /** Current assignment and its package, after registration and slot ownership. */
    suspend fun prepare(): JSONObject { enroll(); send(slotRequest("openSlot")); return send(slotRequest("delivery")) }
    suspend fun confirm(revision: Long) { send(slotRequest("confirmDelivery").put("revision", revision)) }
    suspend fun select(dashboardId: String, params: JSONObject = JSONObject()) {
        enroll(); send(slotRequest("openSlot"))
        val current = send(slotRequest("selectionState"))
        send(slotRequest("select").put("expected", current.getLong("revision"))
            .put("assignment", JSONObject().put("dashboardId", dashboardId).put("params", params).put("presentation", JSONObject())))
    }
}

/** Kotlin port of `engine/shared/client.js`, scoped to one delivered dashboard. Main-thread confined. */
internal class EngineConnection(private val transport: DashboardTransport, private val context: JSONObject, private val onSnapshot: () -> Unit) {
    class Failure(message: String, val connection: Boolean) : Exception(message)
    private val client = "android-" + UUID.randomUUID()
    private var epoch = 1L
    var incarnation: String? = null; private set
    private var revision = -1L
    val values = LinkedHashMap<String, JSONObject>()
    val actions = LinkedHashMap<String, JSONObject>()
    private var resources = setOf<String>()
    private val subscribed = LinkedHashSet<String>()
    private var active = false
    var ready = false; private set
    private var busy = false
    private var ever = false
    private var nextTry = 0L
    var stopped = false
    var status by mutableStateOf(""); private set
    /** Set when the server reports that this dashboard was revoked or replaced. */
    var revoked: String? = null; private set

    private suspend fun call(op: String, args: JSONObject = JSONObject(), hello: Boolean = false): Any? {
        val sent = epoch; val identity = incarnation
        val reply = transport.engine(JSONObject().put("version", 1).put("client", client).put("epoch", sent)
            .put("incarnation", identity ?: JSONObject.NULL).put("op", op).put("args", args).put("dashboard", context))
        if (stopped || sent != epoch) throw Failure("obsolete request", false)
        if (reply.optInt("version") != 1 || reply.optLong("epoch") != sent) throw Failure("invalid response identity", true)
        if (!hello && reply.optString("incarnation") != identity) { ready = false; throw Failure("server incarnation changed", true) }
        if (reply.has("error")) {
            val error = reply.getString("error")
            if (error == "forbidden" || error == "dashboard_changed") revoked = error
            throw Failure(error, false)
        }
        if (hello) { incarnation = reply.getString("incarnation"); revision = -1; subscribed.clear() }
        return reply.opt("value")
    }
    private fun adopt(snapshot: Any?) {
        val s = snapshot as? JSONObject ?: throw Failure("invalid snapshot", true)
        val next = s.getLong("revision")
        if (next < revision) return
        revision = next; values.clear()
        val all: JSONArray = s.getJSONArray("values")
        for (i in 0 until all.length()) all.getJSONObject(i).let { values[it.getString("id")] = it }
        onSnapshot()
    }
    private fun failed(at: Long) {
        if (stopped || at != epoch) return
        ready = false; nextTry = System.currentTimeMillis() + 1000; status = "Disconnected"
    }
    private suspend fun sync() {
        for (id in subscribed.toList()) if (!active || id !in resources) { call("unsubscribe", JSONObject().put("id", id)); subscribed.remove(id) }
        if (active) for (id in resources) if (id !in subscribed) { adopt(call("subscribe", JSONObject().put("id", id))); subscribed.add(id) }
    }
    suspend fun tick() {
        if (busy || stopped || System.currentTimeMillis() < nextTry) return
        busy = true
        var at = epoch
        try {
            if (!ready) {
                if (!ever) status = "Connecting…"
                at = ++epoch
                adopt(call("hello", hello = true)); sync()
                for (id in actions.keys.toList()) actions[id] = call("status", JSONObject().put("actionId", id)) as JSONObject
                adopt(call("snapshot")); ready = true; sync()
                ever = true; status = ""
            } else { sync(); adopt(call(if (active) "poll" else "snapshot")) }
        } catch (error: Exception) { failed(at) } finally { busy = false }
    }
    suspend fun configure(resources: Set<String>, active: Boolean) {
        this.resources = resources; this.active = active
        if (ready && !busy) { val at = epoch; try { sync() } catch (error: Failure) { if (error.connection) failed(at); throw error } }
    }
    suspend fun read(id: String): JSONObject {
        if (!ready) throw Failure("disconnected", true)
        val at = epoch
        try { return call("read", JSONObject().put("id", id)) as JSONObject } catch (error: Failure) { if (error.connection) failed(at); throw error }
    }
    private suspend fun action(op: String, args: JSONObject, actionId: String): JSONObject {
        if (!ready) throw Failure("disconnected", true)
        if (actions.size >= 128) throw Failure("action tracking limit", false)
        val at = epoch
        actions[actionId] = JSONObject().put("status", "unknown").put("actionId", actionId)
        try {
            val result = call(op, args.put("actionId", actionId)) as JSONObject
            actions[actionId] = result
            if (result.optString("status") == "failed") throw Failure(result.optJSONObject("outcome")?.optString("error") ?: "action failed", false)
            return result
        } catch (error: Failure) { if (error.connection) failed(at); throw error }
    }
    suspend fun write(resource: String, wire: Any, actionId: String): JSONObject {
        val current = values[resource] ?: throw Failure("value unavailable: $resource", false)
        return action("write", JSONObject().put("id", resource).put("expected", current.getLong("revision")).put("value", wire), actionId)
    }
    suspend fun run(resource: String, actionId: String) = action("run", JSONObject().put("id", resource), actionId)
    fun close() { stopped = true; epoch++ }
}

