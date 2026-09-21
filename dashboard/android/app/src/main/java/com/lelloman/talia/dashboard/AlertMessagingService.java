package com.lelloman.talia.dashboard;
public final class AlertMessagingService extends com.google.firebase.messaging.FirebaseMessagingService {
 @Override public void onNewToken(String token){AlertPush.token(this,token);}
 @Override public void onMessageReceived(com.google.firebase.messaging.RemoteMessage message){AlertPush.receive(this,message.getData());}
}
