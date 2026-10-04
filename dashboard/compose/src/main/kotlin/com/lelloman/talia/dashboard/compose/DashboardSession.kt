package com.lelloman.talia.dashboard.compose

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** What the dashboard screen shows. [tree] is a resolved, validated UI tree from `TaliaUI.resolve`. */
sealed interface DashboardState {
    data object Loading : DashboardState
    data class Ready(val dashboardId: String, val tree: JSONObject?) : DashboardState
    data class Failed(val message: String) : DashboardState
}

data class DashboardCatalog(val dashboards: List<String>, val defaultDashboard: String?)

/**
 * Runs one delivered dashboard: package validation and UI resolution in the trusted UI context,
 * the dashboard ViewModel in the guest context, and engine I/O restricted to the package grants.
 * Confined to the main thread; QuickJS work hops to its dedicated thread.
 */
class DashboardSession(context: Context, private val transport: DashboardTransport) {
    private val assets = context.applicationContext.assets
    private val registry = ClientRegistry(context.applicationContext, transport)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val scripts by lazy { listOf("value.js", "ui.js", "vm.js").associateWith { name -> assets.open("dashboard/$name").bufferedReader().use { it.readText() } } }

    var state by mutableStateOf<DashboardState>(DashboardState.Loading); private set
    var catalog by mutableStateOf<DashboardCatalog?>(null); private set
    val connectionStatus: String get() = connection?.status ?: ""

    private var connection: EngineConnection? = null
    private var grants = JSONObject()
    private var generation = 0
    private var lastRequest = 0L
    private var width = 360f
    private var paused = false
    private var loop: Job? = null
    private val subscriptions = LinkedHashMap<String, Subscription>()
    private class Subscription(val resource: String) { var ready = false; var key: String? = null }

    private fun quote(text: String) = JSONObject.quote(text)

    suspend fun refreshCatalog(): DashboardCatalog {
        val reply = transport.dashboards(JSONObject().put("op", "catalog"))
        if (reply.has("error")) throw DashboardServerError(reply.getString("error"))
        val value = reply.optJSONObject("value") ?: reply
        val list = value.optJSONArray("dashboards") ?: JSONArray()
        return DashboardCatalog((0 until list.length()).map { list.getJSONObject(it).getString("id") },
            value.optString("defaultDashboard").ifEmpty { null }).also { catalog = it }
    }

    /** Opens [dashboardId], or the client's current assignment when null. */
    fun open(dashboardId: String? = null) {
        val stamp = ++generation
        stopGuest()
        state = DashboardState.Loading
        scope.launch {
            try {
                if (dashboardId != null) registry.select(dashboardId)
                val delivery = registry.prepare()
                val assignment = delivery.getJSONObject("assignment")
                val pkg = delivery.getJSONObject("package")
                if (stamp != generation) return@launch
                val s = scripts
                QuickJs.eval(s.getValue("value.js") + "\n" + s.getValue("ui.js") +
                    "\nglobalThis.__pkg=TaliaUI.validatePackage(" + pkg + ");'ok'", ui = true, reset = true)
                if (pkg.getString("id") != assignment.getString("dashboardId")) throw IllegalStateException("dashboard assignment mismatch")
                registry.confirm(assignment.getLong("revision"))
                if (stamp != generation) return@launch
                grants = pkg.optJSONObject("grants") ?: JSONObject()
                val context = JSONObject().put("id", pkg.getString("id")).put("revision", pkg.getString("revision"))
                    .put("reads", grants.optJSONArray("reads") ?: JSONArray())
                connection = EngineConnection(transport, context) { scope.launch { poll() } }
                lastRequest = 0
                val (_, out) = QuickJs.eval(s.getValue("value.js") + "\n" + s.getValue("vm.js") + "\n" + pkg.getString("viewModel") +
                    "\nTaliaVM.start(" + (pkg.optJSONObject("params") ?: JSONObject()) + ");'ok'", ui = false, reset = true)
                state = DashboardState.Ready(pkg.getString("id"), null)
                requests(out, stamp)
                refresh(stamp)
                loop = scope.launch { while (isActive) { if (!paused) connection?.tick(); checkRevoked(stamp); delay(1000) } }
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { if (stamp == generation) fail(describe(error)) }
        }
    }

    /** Layout width in dp; width rules in dashboard UI are resolved against it. */
    fun resize(widthDp: Float) {
        if (widthDp <= 0f || kotlin.math.abs(widthDp - width) < 0.5f) return
        width = widthDp
        val stamp = generation
        scope.launch { refresh(stamp) }
    }

    /** Dispatches a renderer event to the ViewModel action named by the node. */
    fun dispatch(action: String, target: String, value: Any?) {
        val stamp = generation
        if (state !is DashboardState.Ready || paused) return
        scope.launch { guarded(stamp) {
            val event = JSONObject().put("target", target).put("value", value ?: JSONObject.NULL)
            command("TaliaVM.dispatch(${quote(action)},$event);'ok'", stamp)
        } }
    }

    fun pause() {
        if (paused) return
        paused = true
        val stamp = generation
        scope.launch { guarded(stamp) { QuickJs.eval("TaliaVM.pause();'ok'", ui = false); sync() } }
    }

    fun resume() {
        if (!paused) return
        paused = false
        val stamp = generation
        scope.launch { guarded(stamp) {
            if (state !is DashboardState.Ready) return@guarded
            command("TaliaVM.resume();'ok'", stamp)
            subscriptions.values.forEach { it.key = null }
            sync(); connection?.tick(); poll()
            val outcomes = JSONArray(connection?.actions?.values ?: emptyList<JSONObject>())
            command("TaliaVM.reconcile($outcomes);'ok'", stamp)
        } }
    }

    fun close() { generation++; stopGuest(); scope.cancel() }

    private fun stopGuest() {
        loop?.cancel(); loop = null
        connection?.close(); connection = null
        subscriptions.clear(); grants = JSONObject()
    }

    private fun fail(message: String) {
        stopGuest()
        state = DashboardState.Failed(message)
        scope.launch { runCatching { QuickJs.stop() } }
    }

    private fun describe(error: Exception): String = when {
        error is DashboardServerError && error.code == "forbidden" -> "You don't have access to this dashboard."
        error is DashboardServerError -> "The server rejected the dashboard request (${error.code})."
        error is QuickJs.GuestFailure -> "Dashboard stopped — ${error.message}"
        else -> error.message ?: "Could not load the dashboard."
    }

    private suspend fun guarded(stamp: Int, block: suspend () -> Unit) {
        if (stamp != generation) return
        try { block() } catch (cancelled: CancellationException) { throw cancelled } catch (error: Exception) { if (stamp == generation) fail(describe(error)) }
    }

    private fun checkRevoked(stamp: Int) {
        val reason = connection?.revoked ?: return
        if (stamp != generation) return
        fail(if (reason == "dashboard_changed") "Dashboard updated. Reload to continue." else "Access to this dashboard has been removed.")
    }

    /** Evaluates a ViewModel command, then services its bridge requests and re-renders. */
    private suspend fun command(source: String, stamp: Int) {
        val (_, out) = QuickJs.eval(source, ui = false)
        if (stamp != generation) return
        requests(out, stamp)
        refresh(stamp)
    }

    private suspend fun refresh(stamp: Int) {
        if (stamp != generation || state !is DashboardState.Ready) return
        val (snapshot, _) = QuickJs.eval("TaliaValue.stringify(TaliaVM.snapshot())", ui = false)
        if (stamp != generation) return
        val (resolved, _) = QuickJs.eval("(()=>{const s=TaliaValue.parse(${quote(snapshot)});if(s.failure)return JSON.stringify({failure:String(s.failure)});" +
            "return JSON.stringify({tree:TaliaUI.resolve(__pkg.ui,s.state,{definitions:__pkg.definitions||{},width:$width,scale:1,params:__pkg.params||{}})});})()", ui = true)
        if (stamp != generation) return
        val result = JSONObject(resolved)
        if (result.has("failure")) { fail("Dashboard stopped — " + result.getString("failure")); return }
        state = DashboardState.Ready((state as DashboardState.Ready).dashboardId, result.getJSONObject("tree"))
    }

    private fun allowed(kind: String, id: String) {
        val list = grants.optJSONArray(kind) ?: JSONArray()
        if ((0 until list.length()).none { list.getString(it) == id }) throw IllegalStateException("resource grant")
    }

    private suspend fun sync() {
        val resources = subscriptions.values.map { it.resource }.toSet()
        connection?.configure(resources, !paused && subscriptions.isNotEmpty())
    }

    /** Services guest bridge requests in order, mirroring the web DurableBridge. */
    private suspend fun requests(out: List<JSONObject>, stamp: Int) {
        for (r in out) {
            if (stamp != generation || paused) return
            val id = r.getLong("id")
            if (id <= lastRequest) { fail("Dashboard stopped — bridge replay"); return }
            lastRequest = id
            scope.launch { guarded(stamp) {
                try {
                    when (r.getString("op")) {
                        "subscribe" -> {
                            val resource = r.getString("value"); allowed("reads", resource)
                            if (subscriptions.size >= 16) throw IllegalStateException("subscription budget")
                            val key = "s$id"; subscriptions[key] = Subscription(resource); sync()
                            reply("{id:$id,value:${quote(key)}}", stamp)
                            subscriptions[key]?.ready = true; poll()
                        }
                        "unsubscribe" -> {
                            if (subscriptions.remove(r.getString("value")) == null) throw IllegalStateException("subscription ownership")
                            sync(); reply("{id:$id,value:null}", stamp)
                        }
                        "read" -> {
                            val resource = r.getString("value"); allowed("reads", resource)
                            val value = connection?.read(resource) ?: throw IllegalStateException("disconnected")
                            reply("{id:$id,valueWire:TaliaValue.encode((v=>({...v,hasValue:v.hasValue??v.has_value,value:TaliaValue.decode(v.value)}))($value))}", stamp)
                        }
                        "write" -> {
                            val request = r.optJSONObject("value") ?: throw IllegalStateException("write value")
                            val resource = request.optString("id", "value"); allowed("writes", resource)
                            val result = connection?.write(resource, request.get("wire"), "a-" + UUID.randomUUID()) ?: throw IllegalStateException("disconnected")
                            reply("{id:$id,value:$result}", stamp)
                        }
                        "run" -> {
                            val resource = r.getString("value"); allowed("runs", resource)
                            val result = connection?.run(resource, "a-" + UUID.randomUUID()) ?: throw IllegalStateException("disconnected")
                            reply("{id:$id,value:$result}", stamp)
                        }
                        else -> { fail("Dashboard stopped — operation not granted"); return@guarded }
                    }
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    if (stamp == generation && !paused) reply("{id:$id,error:${quote(error.message ?: error.toString())}}", stamp)
                }
            } }
        }
    }

    /** Delivers a message object (a JS expression) to the guest. */
    private suspend fun reply(message: String, stamp: Int) {
        if (stamp != generation || paused) return
        command("TaliaVM.receive(JSON.stringify($message));'ok'", stamp)
    }

    /** Pushes changed samples to ready subscriptions; samples stay TaliaValue-encoded until inside the guest. */
    private suspend fun poll() {
        val stamp = generation
        val engine = connection ?: return
        if (paused || !engine.ready) return
        for ((event, subscription) in subscriptions.toList()) {
            if (!subscription.ready) continue
            val sample = engine.values[subscription.resource] ?: continue
            val key = engine.incarnation + ":" + sample
            if (key == subscription.key) continue
            subscription.key = key
            reply("{event:${quote(event)},valueWire:TaliaValue.encode((v=>({...v,value:TaliaValue.decode(v.value)}))($sample))}", stamp)
        }
    }
}
