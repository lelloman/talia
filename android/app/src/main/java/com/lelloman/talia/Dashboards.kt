package com.lelloman.talia

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.LifecycleEventObserver
import com.lelloman.lellodesign.*
import com.lelloman.talia.dashboard.compose.DashboardContent
import com.lelloman.talia.dashboard.compose.DashboardState
import kotlinx.coroutines.launch

/** Four-panel mark matching the web dashboard navigation icon. */
internal val DashboardIcon: ImageVector by lazy {
    ImageVector.Builder("Dashboard", 24.dp, 24.dp, 24f, 24f).path(fill = SolidColor(androidx.compose.ui.graphics.Color.Black)) {
        listOf(3f to 3f, 13f to 3f, 3f to 13f, 13f to 13f).forEach { (x, y) ->
            moveTo(x + 1.5f, y); lineTo(x + 6.5f, y); quadTo(x + 8f, y, x + 8f, y + 1.5f); lineTo(x + 8f, y + 6.5f)
            quadTo(x + 8f, y + 8f, x + 6.5f, y + 8f); lineTo(x + 1.5f, y + 8f); quadTo(x, y + 8f, x, y + 6.5f)
            lineTo(x, y + 1.5f); quadTo(x, y, x + 1.5f, y); close()
        }
    }.build()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DashboardsScreen(connection: NativeConnection, modifier: Modifier, openSettings: () -> Unit) {
    if (!connection.signedIn) {
        LelloWorkspace(modifier.verticalScroll(rememberScrollState())) {
            LelloState(title = "Connect to see dashboards", description = "Sign in to your Talìa server to view the dashboards shared with you.",
                modifier = Modifier.fillMaxWidth(), icon = { Icon(DashboardIcon, null) },
                action = { LelloButton(openSettings) { Text("Connect your server") } })
        }
        return
    }
    val session = connection.dashboards
    val context = LocalContext.current
    val preferences = remember { context.getSharedPreferences("dashboards", android.content.Context.MODE_PRIVATE) }
    var selected by remember { mutableStateOf(preferences.getString("selected", null)) }
    var catalogError by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun choose(id: String) {
        selected = id; preferences.edit().putString("selected", id).apply(); session.open(id)
    }
    suspend fun load(reopen: Boolean) {
        try {
            val catalog = session.refreshCatalog(); catalogError = null
            val target = selected?.takeIf { it in catalog.dashboards } ?: catalog.defaultDashboard?.takeIf { it in catalog.dashboards } ?: catalog.dashboards.firstOrNull()
            if (target == null) return
            if (reopen || target != selected || session.state is DashboardState.Failed) choose(target)
        } catch (error: Exception) {
            catalogError = when {
                error is ApiFailure && error.status in listOf(404, 405) -> "This server needs the dashboards update."
                error is ApiFailure && error.status == 401 -> "Your session expired. Sign in again."
                error is RemoteAccessFailure -> error.message
                else -> "Could not load dashboards. Check your connection and try again."
            }
        }
    }
    LaunchedEffect(Unit) { load(reopen = session.state !is DashboardState.Ready) }
    // Only the visible, started screen keeps the ViewModel and its subscriptions running.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle, session) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_START) session.resume()
            if (event == Lifecycle.Event.ON_STOP) session.pause()
        }
        lifecycle.addObserver(observer)
        if (lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) session.resume()
        onDispose { lifecycle.removeObserver(observer); session.pause() }
    }
    PullToRefreshBox(isRefreshing = refreshing, onRefresh = { scope.launch { refreshing = true; load(reopen = true); refreshing = false } }, modifier = modifier) {
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val gutter = if (maxWidth < 600.dp) 16.dp else 24.dp
            LaunchedEffect(maxWidth) { session.resize((maxWidth - gutter * 2).value) }
            Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = gutter, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp)) {
                val catalog = session.catalog
                if (catalog != null && catalog.dashboards.size > 1) {
                    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        catalog.dashboards.forEach { id -> LelloFilterChip(selected = id == selected, onClick = { if (id != selected) choose(id) }, label = { Text(id) }) }
                    }
                }
                catalogError?.let { LelloAlert(it, Modifier.fillMaxWidth(), tone = LelloTone.Error) }
                session.connectionStatus.ifEmpty { null }?.let { LelloConnectionStatusLine(it) }
                when (val state = session.state) {
                    is DashboardState.Loading -> if (catalog?.dashboards?.isEmpty() == true) {
                        LelloState(title = "No dashboards yet", description = "An admin can create a dashboard and share it with you. It will appear here.",
                            modifier = Modifier.fillMaxWidth(), icon = { Icon(DashboardIcon, null) })
                    } else if (catalogError == null) Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = androidx.compose.ui.Alignment.Center) { CircularProgressIndicator() }
                    is DashboardState.Failed -> LelloAlert(state.message, Modifier.fillMaxWidth(), tone = LelloTone.Error) {
                        LelloOutlinedButton({ scope.launch { load(reopen = true) } }) { Text("Reload dashboard") }
                    }
                    is DashboardState.Ready -> state.tree?.let { tree ->
                        DashboardContent(tree, onEvent = session::dispatch, modifier = Modifier.fillMaxWidth())
                    }
                }
            }
        }
    }
}

@Composable
private fun LelloConnectionStatusLine(status: String) {
    Text(status, style = MaterialTheme.typography.bodySmall, color = LocalLelloPalette.current["text-secondary"])
}

/** Title for the app bar: the open dashboard, or the section name. */
internal fun dashboardTitle(connection: NativeConnection): String =
    (connection.takeIf { it.signedIn }?.dashboards?.state as? DashboardState.Ready)?.dashboardId ?: "Dashboards"
