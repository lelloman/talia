package com.lelloman.talia

import android.os.Bundle
import android.content.Intent
import android.net.Uri
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.delay
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Image
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import com.lelloman.lellodesign.*
import com.lelloman.paravoidandroid.runtime.ParavoidUpdates

class MainActivity : ComponentActivity() {
    private var notificationDestination by mutableStateOf<Intent?>(null)
    private val notificationPermission = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) androidx.lifecycle.ViewModelProvider(this)[NativeConnection::class.java].enableNotifications()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handleGatewayIntent(intent)
        if (intent?.hasExtra("notificationPage") == true) notificationDestination = Intent(intent)
        setContent { TaliaApp() }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleGatewayIntent(intent)
        if (intent?.hasExtra("notificationPage") == true) notificationDestination = Intent(intent)
    }
    private fun handleGatewayIntent(intent: Intent?) {
        val uri = intent?.data?.toString()?.let { runCatching { java.net.URI(it) }.getOrNull() } ?: return
        if (!HomelabGateway.isCallback(uri)) return
        intent.data = null
        androidx.lifecycle.ViewModelProvider(this)[NativeConnection::class.java].completeGateway(uri) { url ->
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addCategory(Intent.CATEGORY_BROWSABLE))
        }
    }

    @Composable
    private fun TaliaApp() {
        val connection: NativeConnection = viewModel()
        val lifecycle = LocalLifecycleOwner.current.lifecycle
        LaunchedEffect(connection, lifecycle) {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    if (!connection.gateway.pending && (connection.pending || connection.signedIn)) connection.refresh()
                    delay(if (connection.pending) 2000 else 30000)
                }
            }
        }
        val openBrowser: (String) -> Unit = { url ->
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addCategory(Intent.CATEGORY_BROWSABLE))
        }
        val preferences = remember { getSharedPreferences("appearance", MODE_PRIVATE) }
        var appearance by remember { mutableStateOf(runCatching {
            LelloAppearance.valueOf(preferences.getString("mode", "System")!!)
        }.getOrDefault(LelloAppearance.System)) }
        var page by rememberSaveable { mutableStateOf("overview") }
        LaunchedEffect(notificationDestination) {
            notificationDestination?.let { destination ->
                page = if (destination.getStringExtra("notificationPage") == "reports") "reports" else "overview"
                connection.openNotification(destination.getStringExtra("notificationReport").orEmpty(), destination.getStringExtra("notificationRun").orEmpty())
                notificationDestination = null
            }
        }
        val pageState = androidx.compose.runtime.saveable.rememberSaveableStateHolder()
        val dark = when (appearance) {
            LelloAppearance.Light -> false
            LelloAppearance.Dark -> true
            LelloAppearance.System -> isSystemInDarkTheme()
        }
        SideEffect {
            androidx.core.view.WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
        val destinations = listOf(
            LelloDestination("overview", "Overview") { Icon(Icons.Default.Home, null) },
            LelloDestination("dashboards", "Dashboards") { Icon(DashboardIcon, null) },
            LelloDestination("reports", "Reports") { Icon(Icons.AutoMirrored.Filled.List, null) },
            LelloDestination("automation", "Automation") { Icon(Icons.Default.DateRange, null) },
            LelloDestination("chats", "Chats") { Icon(Icons.Default.Email, null) },
            LelloDestination("settings", "Settings") { Icon(Icons.Default.Settings, null) },
        )
        LelloTheme(product = "blue", dark = dark) {
            // An open chat is a full-screen destination outside the navigation scaffold.
            if (page == "chats" && connection.signedIn && connection.chats.open != null) {
                ChatConversationScreen(connection) { page = "reports" }
                return@LelloTheme
            }
            LelloScaffold(
                productName = "Talìa", title = when {
                    page == "reports" && connection.reportSelected.isNotEmpty() -> connection.reportSelected
                    page == "dashboards" -> dashboardTitle(connection)
                    else -> destinations.first { it.id == page }.label
                },
                destinations = destinations, selectedId = page, onNavigate = { page = it },
                mobileNavigation = LelloMobileNavigation.DrawerAndBottom,
                bottomDestinations = destinations.filter { it.id in setOf("overview", "dashboards", "reports", "chats") },
                logo = { Image(painterResource(R.drawable.ic_talia), "Talìa", Modifier.size(32.dp)) },
                account = { compact -> LelloAccount(connection.name, { page = "settings" }, compact = compact) },
            ) { insets ->
                pageState.SaveableStateProvider(page) {
                    if (page == "reports") {
                        ReportsScreen(connection, Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets).imePadding(),
                            investigate = { run -> connection.chats.investigate(run); page = "chats" }) { page = "settings" }
                    } else if (page == "chats") {
                        ChatsScreen(connection, Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets), openReports = { page = "reports" }) { page = "settings" }
                    } else if (page == "dashboards") {
                        DashboardsScreen(connection, Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets)) { page = "settings" }
                    } else LelloWorkspace(Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets)
                        .imePadding().verticalScroll(rememberScrollState())) {
                        if (page == "settings") {
                            ConnectionPanel(connection, openBrowser)
                            ConnectionSettings(connection, openBrowser)
                            LelloSettingsSection("Notifications") {
                                Text(connection.notificationsMessage ?: "Receive report results and incident changes through LelloStore.")
                                Button(enabled = connection.signedIn && !connection.busy, onClick = {
                                    if (connection.notificationsEnabled) connection.disableNotifications()
                                    else if (android.os.Build.VERSION.SDK_INT >= 33) notificationPermission.launch(android.Manifest.permission.POST_NOTIFICATIONS)
                                    else connection.enableNotifications()
                                }) { Text(if (connection.notificationsEnabled) "Disable notifications" else "Enable notifications") }
                            }
                            LelloSettingsSection("Appearance") {
                                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                        Text("Theme", style = MaterialTheme.typography.titleMedium)
                                        Muted(when (appearance) {
                                            LelloAppearance.System -> "Follow your device settings"
                                            LelloAppearance.Light -> "Light appearance"
                                            LelloAppearance.Dark -> "Dark appearance"
                                        })
                                    }
                                    LelloAppearanceSelector(appearance, { choice ->
                                        appearance = choice
                                        preferences.edit().putString("mode", choice.name).apply()
                                    })
                                }
                            }
                            LelloSettingsSection("App updates") {
                                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Text("Talìa ${BuildConfig.APP_VERSION}", style = MaterialTheme.typography.titleMedium)
                                    Muted("Keep Talìa up to date with the latest improvements.")
                                }
                                if (BuildConfig.FLAVOR == "paravoidAndroid") {
                                    LelloOutlinedButton({ ParavoidUpdates.get().openControls(this@MainActivity) }) {
                                        Text("Manage app updates")
                                    }
                                }
                            }
                        } else if (page == "overview") {
                            Overview(connection) { page = "settings" }
                        } else {
                            LelloState(
                                title = "${destinations.first { it.id == page }.label} is coming next",
                                description = when (page) {
                                    else -> "Schedules and alert rules will live here. They aren’t available in the app yet."
                                },
                                modifier = Modifier.fillMaxWidth(),
                                icon = { destinations.first { it.id == page }.icon() },
                                action = { LelloOutlinedButton({ page = "overview" }) { Text("Back to Overview") } },
                            )
                        }
                    }
                }
            }
        }
    }
}
