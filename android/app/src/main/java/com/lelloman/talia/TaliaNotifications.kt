package com.lelloman.talia

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import com.lelloman.store.notifications.client.NotificationClient
import com.lelloman.store.notifications.client.NotificationReceiverService

/** Shared by the UI and receiver, including in the Paravoid payload process. */
internal object TaliaNotifications {
    @Volatile private var instance: NotificationClient? = null
    @Synchronized fun client(context: Context): NotificationClient {
        instance?.let { return it }
        val app = context.applicationContext
        val manager = app.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("reports", "Reports", NotificationManager.IMPORTANCE_DEFAULT))
        manager.createNotificationChannel(NotificationChannel("incidents", "Incidents", NotificationManager.IMPORTANCE_HIGH))
        return NotificationClient(app, BuildConfig.STORE_PACKAGE, BuildConfig.STORE_CERTIFICATES.split(',').filter { it.isNotBlank() }.toSet(), TaliaNotificationReceiver::class.java.name) { envelope ->
            val message = envelope.getJSONObject("message")
            val payload = message.getJSONObject("payload")
            val incident = message.getString("type") == "incident.state"
            if (incident && (!payload.optBoolean("active", payload.optInt("active") == 1) || payload.optBoolean("acknowledged", payload.optInt("acknowledged") == 1))) null
            else {
                val intent = Intent(app, MainActivity::class.java).putExtra("notificationPage", if (incident) "overview" else "reports")
                    .putExtra("notificationRun", payload.optString("run_id")).putExtra("notificationReport", payload.optString("report_id"))
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                val open = PendingIntent.getActivity(app, envelope.getString("delivery_id").hashCode(), intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
                Notification.Builder(app, if (incident) "incidents" else "reports")
                    .setSmallIcon(R.drawable.ic_talia).setContentTitle(payload.optString("title", "Talìa"))
                    .setContentText(payload.optString("summary")).setStyle(Notification.BigTextStyle().bigText(payload.optString("summary")))
                    .setContentIntent(open).setAutoCancel(true).setOnlyAlertOnce(true)
                    .setVisibility(Notification.VISIBILITY_PRIVATE).build()
            }
        }.also { instance = it }
    }
}
class TaliaNotificationReceiver : NotificationReceiverService() {
    override val client get() = TaliaNotifications.client(this)
}
