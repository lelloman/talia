package com.lelloman.talia

import android.content.SharedPreferences
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.lelloman.lellodesign.*
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.text.DateFormat
import java.util.UUID

/**
 * Server-owned chat sessions (`/native/chats`). Drafts and unconfirmed sends persist
 * per session; an unconfirmed send keeps its request ID so a retry never duplicates.
 */
internal class ChatModel(
    private val call: suspend (JSONObject) -> JSONObject,
    private val prefs: SharedPreferences,
    private val scope: CoroutineScope,
) {
    var sessions by mutableStateOf<List<JSONObject>>(emptyList()); private set
    var loaded by mutableStateOf(false); private set
    /** null: session list; "": a new chat; otherwise the open session ID. */
    var open by mutableStateOf<String?>(null); private set
    var requests by mutableStateOf<List<JSONObject>>(emptyList()); private set
    var message by mutableStateOf<String?>(null)
    var forbidden by mutableStateOf(false); private set
    var busy by mutableStateOf(false); private set
    var draft by mutableStateOf(""); private set
    var unsent by mutableStateOf<JSONObject?>(null); private set
    /** Report run attached to the next new chat; sent only when the session is created. */
    var attached by mutableStateOf<String?>(null); private set
    val running get() = requests.any { it.optString("status") in setOf("queued", "running") }
    val title get() = sessions.firstOrNull { it.optString("id") == open }?.optString("title")

    private fun key(kind: String) = "chat.$kind.${open?.ifEmpty { "new" } ?: "new"}"
    private suspend fun rpc(op: String, args: JSONObject = JSONObject()) = call(JSONObject().put("op", op).put("args", args))
    private fun describe(error: Throwable): String? = when {
        error is ApiFailure && error.status == 403 -> { forbidden = true; null }
        error is ApiFailure && error.status in listOf(404, 405) && !loaded -> "This server needs the chat update."
        error is ApiFailure && error.status == 404 -> "This chat is no longer available."
        error is ApiFailure && error.status == 429 -> "Too many requests are running. Wait for an answer, or stop one, then try again."
        error is ApiFailure && error.status == 401 -> "Your session expired. Sign in again."
        error is RemoteAccessFailure -> error.message
        else -> "Could not reach Talìa. Check your connection and try again."
    }
    private fun loadDraft() {
        unsent = prefs.getString(key("unsent"), null)?.let(::JSONObject)
        draft = unsent?.optString("text") ?: prefs.getString(key("draft"), "").orEmpty()
    }
    fun edit(text: String) {
        draft = text
        prefs.edit().putString(key("draft"), text).apply()
    }
    suspend fun refreshList() {
        try {
            val v = rpc("list").optJSONArray("sessions") ?: JSONArray()
            sessions = (0 until v.length()).map { v.getJSONObject(it) }
            loaded = true; forbidden = false
        } catch (c: CancellationException) { throw c } catch (e: Exception) { message = describe(e) }
    }
    suspend fun refreshSession() {
        val id = open?.takeIf { it.isNotEmpty() } ?: return
        try {
            val v = rpc("get", JSONObject().put("session", id)).optJSONArray("requests") ?: JSONArray()
            if (open == id) requests = (0 until v.length()).map { v.getJSONObject(it) }
        } catch (c: CancellationException) { throw c } catch (e: Exception) {
            if (e is ApiFailure && e.status == 404) { open = null; requests = emptyList(); refreshList() }
            message = describe(e)
        }
    }
    fun openSession(id: String?) {
        attached = null; open = id; requests = emptyList(); message = null; loadDraft()
        if (!id.isNullOrEmpty()) scope.launch { refreshSession() }
    }
    fun close() { open = null; message = null; scope.launch { refreshList() } }
    fun send() {
        val text = draft.trim()
        if (text.isEmpty() || busy) return
        val report = if (open.isNullOrEmpty()) attached else null
        val pending = unsent?.takeIf { it.optString("text") == text && it.optString("report").ifEmpty { null } == report }
            ?: JSONObject().put("requestId", UUID.randomUUID().toString()).put("text", text).apply { report?.let { put("report", it) } }
        // Persist before dispatch so a lost response can be retried as the same request.
        unsent = pending; prefs.edit().putString(key("unsent"), pending.toString()).apply()
        busy = true; message = null
        scope.launch {
            try {
                val result = if (open.isNullOrEmpty()) rpc("create", pending)
                    else rpc("send", JSONObject(pending.toString()).put("session", open).apply { remove("report") })
                prefs.edit().remove(key("unsent")).remove(key("draft")).apply()
                unsent = null; draft = ""
                if (open.isNullOrEmpty()) { open = result.getString("session"); attached = null }
                refreshSession(); refreshList()
            } catch (c: CancellationException) { throw c } catch (e: Exception) {
                // An explicit rejection admitted nothing; a transport failure is ambiguous.
                if (e is ApiFailure && e.status in listOf(400, 403, 404, 429)) { prefs.edit().remove(key("unsent")).apply(); unsent = null }
                message = describe(e)?.let { if (e is IOException) "Message not confirmed. Retry sends it once; it won't be duplicated." else it }
            } finally { busy = false }
        }
    }
    /** Investigate from Reports: a new chat with the run attached and an editable first message. */
    fun investigate(run: String) {
        openSession(""); attached = run
        // Repeated taps reuse an unconfirmed create for the same run instead of starting another.
        if (unsent?.optString("report") != run) {
            prefs.edit().remove(key("unsent")).apply(); unsent = null
            edit("Investigate report run $run: what needs attention, and why?")
        }
    }
    fun cancelInvestigation() { prefs.edit().remove(key("unsent")).remove(key("draft")).apply(); unsent = null; draft = ""; attached = null; open = null }
    fun again(request: JSONObject) { prefs.edit().remove(key("unsent")).apply(); unsent = null; edit(request.optString("text")) }
    private fun act(block: suspend () -> Unit) {
        if (busy) return
        busy = true; message = null
        scope.launch {
            try { block() } catch (c: CancellationException) { throw c } catch (e: Exception) { message = describe(e) } finally { busy = false }
        }
    }
    fun stop() = act { rpc("stop", JSONObject().put("session", open)); refreshSession(); refreshList() }
    fun rename(name: String) = act { rpc("rename", JSONObject().put("session", open).put("title", name)); refreshList() }
    fun delete() = act {
        rpc("delete", JSONObject().put("session", open))
        prefs.edit().remove(key("draft")).remove(key("unsent")).apply()
        open = null; requests = emptyList(); refreshList()
    }
}

@Composable
internal fun ChatsScreen(connection: NativeConnection, modifier: Modifier, openReports: () -> Unit = {}, openSettings: () -> Unit) {
    if (!connection.signedIn) {
        LelloWorkspace(modifier.verticalScroll(rememberScrollState())) {
            LelloState(title = "Connect to chat with Talìa", description = "Sign in to your Talìa server to ask about your homelab.",
                modifier = Modifier.fillMaxWidth(), icon = { Icon(Icons.Default.Email, null) },
                action = { LelloButton(openSettings) { Text("Connect your server") } })
        }
        return
    }
    val chats = connection.chats
    ChatPolling(chats)
    if (chats.forbidden) {
        LelloWorkspace(modifier) {
            LelloState(title = "Chat unavailable", description = "Chat requires administrator access.",
                modifier = Modifier.fillMaxWidth(), icon = { Icon(Icons.Default.Email, null) })
        }
    } else ChatList(chats, modifier)
}

/** Polls quickly only while an answer is being worked on, and only while visible. */
@Composable
private fun ChatPolling(chats: ChatModel) {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(chats, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            chats.refreshList(); chats.refreshSession()
            while (true) {
                delay(if (chats.running) 1500 else 10000)
                chats.refreshSession()
                if (!chats.running) chats.refreshList()
            }
        }
    }
}

/** An open conversation is its own full-screen destination, outside the navigation scaffold. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ChatConversationScreen(connection: NativeConnection, openReports: () -> Unit) {
    val chats = connection.chats
    val palette = LocalLelloPalette.current
    var menu by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf(false) }
    ChatPolling(chats)
    BackHandler { chats.close() }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(chats.title ?: "New chat", Modifier.semantics { heading() }, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { IconButton({ chats.close() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back to chats") } },
                actions = {
                    if (chats.running) LelloTextButton({ chats.stop() }, enabled = !chats.busy) { Text("Stop") }
                    if (!chats.open.isNullOrEmpty()) Box {
                        IconButton({ menu = true }) { Icon(Icons.Default.MoreVert, "Chat actions") }
                        DropdownMenu(menu, { menu = false }) {
                            DropdownMenuItem({ Text("Rename") }, { menu = false; renaming = chats.title.orEmpty() })
                            DropdownMenuItem({ Text("Delete") }, { menu = false; deleting = true })
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = palette["surface"]),
            )
        },
        containerColor = palette["background"],
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { inner ->
        Conversation(chats, Modifier.fillMaxSize().padding(inner).consumeWindowInsets(inner), openReports)
    }
    renaming?.let { name ->
        AlertDialog({ renaming = null }, title = { Text("Rename chat") },
            text = { LelloTextField(name, { renaming = it.take(120) }, label = { Text("Chat title") }) },
            confirmButton = { LelloTextButton({ if (name.isNotBlank()) { chats.rename(name.trim()); renaming = null } }) { Text("Save") } },
            dismissButton = { LelloTextButton({ renaming = null }) { Text("Cancel") } })
    }
    if (deleting) AlertDialog({ deleting = false }, title = { Text("Delete this chat?") },
        text = { Text("The conversation will be removed from your chats. Running work stops.") },
        confirmButton = { LelloTextButton({ deleting = false; chats.delete() }) { Text("Delete chat", color = palette["error"]) } },
        dismissButton = { LelloTextButton({ deleting = false }) { Text("Cancel") } })
}

@Composable
private fun ChatList(chats: ChatModel, modifier: Modifier) {
    val palette = LocalLelloPalette.current
    Box(modifier) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, top = 16.dp, end = 16.dp, bottom = 96.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        chats.message?.let { item { LelloAlert(it, Modifier.fillMaxWidth().padding(bottom = 8.dp), tone = LelloTone.Error) } }
        if (chats.loaded && chats.sessions.isEmpty()) item {
            LelloState(title = "Ask Talìa about your homelab", description = "Talìa reads current monitoring values, history, alerts and reports, and queries approved diagnostic sources. It never changes anything.",
                modifier = Modifier.fillMaxWidth().padding(top = 24.dp), icon = { Icon(Icons.Default.Email, null) })
        }
        items(chats.sessions, key = { it.optString("id") }) { s ->
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable { chats.openSession(s.optString("id")) }.padding(horizontal = 12.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(s.optString("title"), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (s.optBoolean("running")) Text("Running", style = MaterialTheme.typography.labelMedium, color = palette["info"])
                    Text(DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(s.optLong("updated")),
                        style = MaterialTheme.typography.bodySmall, color = palette["text-secondary"])
                }
            }
        }
    }
    ExtendedFloatingActionButton(onClick = { chats.openSession("") }, modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp).semantics { contentDescription = "New chat" },
        icon = { Icon(Icons.Default.Add, null) }, text = { Text("New chat") })
    }
}

@Composable
private fun Conversation(chats: ChatModel, modifier: Modifier, openReports: () -> Unit) {
    val palette = LocalLelloPalette.current
    val list = rememberLazyListState()
    LaunchedEffect(chats.requests.size, chats.requests.lastOrNull()?.optString("status")) {
        if (chats.requests.isNotEmpty()) list.animateScrollToItem(chats.requests.size - 1)
    }
    Column(modifier.imePadding()) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (chats.requests.isEmpty()) item {
                LelloState(title = "Ask Talìa about your homelab", description = "Questions can cover current values, history, alerts and report runs. Talìa only reads.",
                    modifier = Modifier.fillMaxWidth().padding(top = 32.dp), icon = { Icon(Icons.Default.Email, null) })
            }
            items(chats.requests, key = { it.optLong("id") }) { r -> Turn(r) { chats.again(r) } }
        }
        chats.attached?.takeIf { chats.open == "" }?.let { run ->
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Report run $run", Modifier.weight(1f), style = MaterialTheme.typography.labelLarge, color = palette["info"],
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                LelloTextButton({ chats.cancelInvestigation(); openReports() }) { Text("Cancel") }
            }
        }
        chats.message?.let { LelloAlert(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp), tone = LelloTone.Error) }
        HorizontalDivider(color = palette["border-subtle"])
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            LelloTextField(chats.draft, { chats.edit(it.take(8000)) }, label = { Text("Message") }, modifier = Modifier.weight(1f),
                singleLine = false)
            FilledIconButton({ chats.send() }, enabled = chats.draft.isNotBlank() && !chats.busy,
                modifier = Modifier.semantics { contentDescription = if (chats.unsent != null) "Retry sending" else "Send" }) {
                if (chats.busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Icon(Icons.AutoMirrored.Filled.Send, null)
            }
        }
    }
}

@Composable
private fun Turn(r: JSONObject, again: () -> Unit) {
    val palette = LocalLelloPalette.current
    val status = r.optString("status")
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Surface(Modifier.align(Alignment.End).widthIn(max = 320.dp), shape = RoundedCornerShape(16.dp, 16.dp, 4.dp, 16.dp), color = palette["primary-container"]) {
            Text(r.optString("text"), Modifier.padding(horizontal = 14.dp, vertical = 10.dp), color = palette["on-primary-container"])
        }
        when (status) {
            "done" -> SelectionContainer { Text(r.optString("answer"), style = MaterialTheme.typography.bodyLarge) }
            "queued", "running" -> Surface(shape = RoundedCornerShape(4.dp, 16.dp, 16.dp, 16.dp), color = palette["surface"],
                border = androidx.compose.foundation.BorderStroke(1.dp, palette["border-subtle"]),
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
                        Text(if (status == "queued") "Waiting to start…" else "Investigating…", style = MaterialTheme.typography.titleSmall)
                    }
                    val steps = r.optJSONArray("steps") ?: JSONArray()
                    for (i in 0 until steps.length()) {
                        val step = steps.getJSONObject(i)
                        val done = step.optBoolean("done")
                        Text((if (done) "✓  " else "○  ") + step.optString("label"), style = MaterialTheme.typography.bodySmall,
                            color = if (done) palette["text-secondary"] else palette["text"])
                    }
                }
            }
            else -> Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                LelloAlert(when (status) {
                    "stopped" -> "Stopped before an answer was ready."
                    "cancelled" -> "Cancelled."
                    else -> r.optString("error").ifEmpty { "The investigation failed." }
                }, Modifier.fillMaxWidth(), tone = if (status == "stopped") LelloTone.Info else LelloTone.Error)
                LelloTextButton(again) { Text("Try again") }
            }
        }
    }
}
