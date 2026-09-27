package com.lelloman.talia

import android.os.Bundle
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
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { TaliaApp() }
    }

    @Composable
    private fun TaliaApp() {
        val preferences = remember { getSharedPreferences("appearance", MODE_PRIVATE) }
        var appearance by remember { mutableStateOf(runCatching {
            LelloAppearance.valueOf(preferences.getString("mode", "System")!!)
        }.getOrDefault(LelloAppearance.System)) }
        var page by rememberSaveable { mutableStateOf("overview") }
        val dark = when (appearance) {
            LelloAppearance.Light -> false
            LelloAppearance.Dark -> true
            LelloAppearance.System -> isSystemInDarkTheme()
        }
        val destinations = listOf(
            LelloDestination("overview", "Overview") { Icon(Icons.Default.Home, null) },
            LelloDestination("reports", "Reports") { Icon(Icons.AutoMirrored.Filled.List, null) },
            LelloDestination("automation", "Automation") { Icon(Icons.Default.DateRange, null) },
            LelloDestination("chats", "Chats") { Icon(Icons.Default.Email, null) },
            LelloDestination("settings", "Settings") { Icon(Icons.Default.Settings, null) },
        )
        LelloTheme(product = "blue", dark = dark) {
            LelloScaffold(
                productName = "Talìa", title = destinations.first { it.id == page }.label,
                destinations = destinations, selectedId = page, onNavigate = { page = it },
                mobileNavigation = LelloMobileNavigation.Drawer,
                logo = { Image(painterResource(R.drawable.ic_talia), "Talìa", Modifier.size(32.dp)) },
                account = { compact -> LelloAccount("Not signed in", { page = "settings" }, compact = compact) },
            ) { insets ->
                Column(Modifier.fillMaxSize().padding(insets).consumeWindowInsets(insets)
                    .verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
                    if (page == "settings") {
                        LelloSection("Appearance") {
                            LelloAppearance.entries.forEach { choice ->
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    RadioButton(appearance == choice, onClick = {
                                        appearance = choice
                                        preferences.edit().putString("mode", choice.name).apply()
                                    })
                                    Text(choice.name)
                                }
                            }
                        }
                        LelloSection("App updates") {
                            Text("Version ${BuildConfig.VERSION_NAME}")
                            if (BuildConfig.FLAVOR == "paravoidAndroid") {
                                Text("Updates are verified and managed by Paravoid.")
                                Button(onClick = { ParavoidUpdates.get().openControls(this@MainActivity) }) {
                                    Text("Manage app updates")
                                }
                            } else Text("Normal APK build. Install a new APK to update.")
                        }
                        LelloSection("Connection") {
                            Text("Not connected. Server sign-in will be added in the next implementation slice.")
                        }
                    } else {
                        LelloSection("Android preview") {
                            Text("The native app shell is ready. ${destinations.first { it.id == page }.label} is not connected to your server yet.")
                            Text("No live service status or results are shown in this build.")
                        }
                        Button(onClick = { page = "settings" }) { Text("Open settings") }
                    }
                }
            }
        }
    }
}
