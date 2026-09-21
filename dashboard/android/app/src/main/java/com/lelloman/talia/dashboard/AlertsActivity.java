package com.lelloman.talia.dashboard;
import android.app.Activity;
import android.os.*;
import android.text.InputType;
import android.view.View;
import android.widget.*;
import org.json.*;
import java.util.UUID;
import java.util.concurrent.*;
/** Native host surface, independent of the dashboard runtime. */
public final class AlertsActivity extends Activity {
 final ExecutorService io=Executors.newSingleThreadExecutor();final Handler main=new Handler(Looper.getMainLooper());
 LinearLayout content;TextView status;int port;boolean foreground,busy;String focusKey;
 interface Work {void run()throws Exception;}
 @Override public void onCreate(Bundle state){super.onCreate(state);port=getIntent().getIntExtra("port",18744);focusKey=getIntent().getStringExtra("alertKey");
  LinearLayout root=new LinearLayout(this);root.setOrientation(1);root.setPadding(24,40,24,24);setContentView(root);
  TextView title=new TextView(this);title.setText("Talìa alerts");title.setTextSize(24);root.addView(title);
  EditText token=new EditText(this);token.setHint("Alert access token");token.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_VARIATION_PASSWORD);root.addView(token);
  Button connect=new Button(this);connect.setText("Connect alerts");root.addView(connect);connect.setOnClickListener(v->{if(token.length()>0){getSharedPreferences("talia-alerts",0).edit().putString("credential",token.getText().toString()).apply();token.setText("");}refresh();});
  status=new TextView(this);status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);root.addView(status);
  Button refresh=new Button(this);refresh.setText("Refresh alerts");root.addView(refresh);refresh.setOnClickListener(v->refresh());
  ScrollView scroll=new ScrollView(this);content=new LinearLayout(this);content.setOrientation(1);scroll.addView(content);root.addView(scroll,new LinearLayout.LayoutParams(-1,0,1));
 }
 void label(String text){TextView view=new TextView(this);view.setText(text);view.setTextSize(16);view.setPadding(0,8,0,8);content.addView(view);}
 void action(String text,Work work){Button button=new Button(this);button.setText(text);content.addView(button);button.setOnClickListener(v->{button.setEnabled(false);io.execute(()->{try{work.run();main.post(this::refresh);}catch(Exception e){main.post(()->{status.setText(e.getMessage());button.setEnabled(true);});}});});}
 void refresh(){if(!foreground||busy)return;busy=true;io.execute(()->{try{JSONObject state=AlertApi.request(this,port,"snapshot",new JSONObject());main.post(()->{busy=false;if(!foreground)return;try{render(state);}catch(Exception e){status.setText("Invalid alert response");}});}catch(Exception e){main.post(()->{busy=false;status.setText(e.getMessage());});}});}
 void render(JSONObject state)throws Exception{content.removeAllViews();status.setText("");JSONArray alerts=state.getJSONArray("alerts");if(alerts.length()==0)label("No alerts");
  for(int i=0;i<alerts.length();i++){JSONObject a=alerts.getJSONObject(i);if(focusKey!=null&&!focusKey.equals(a.getString("key")))continue;String key=a.getString("key");boolean active=a.getBoolean("active");
   label(key+" · "+a.getString("severity")+" · "+(active?"Active":"Resolved")+(a.isNull("acknowledgement")?"":" · Acknowledged"));label(a.getString("message"));
   if(active&&a.isNull("acknowledgement"))action("Acknowledge",()->AlertApi.request(this,port,"acknowledge",new JSONObject().put("key",key).put("occurrence",a.getLong("occurrence")).put("expected",a.getLong("revision"))));
   action("Silence for 1 hour",()->AlertApi.request(this,port,"silence_save",new JSONObject().put("expected",0).put("silence",new JSONObject().put("id",UUID.randomUUID().toString()).put("version",1).put("key",key).put("until",state.getLong("now")+3600000).put("reason","Dashboard maintenance"))));
   action("History",()->{JSONObject history=AlertApi.request(this,port,"history",new JSONObject().put("key",key));main.post(()->{try{JSONArray rows=history.getJSONArray("occurrences");StringBuilder text=new StringBuilder();for(int n=0;n<rows.length();n++){JSONObject h=rows.getJSONObject(n);text.append("Occurrence ").append(h.getLong("occurrence")).append(": ").append(h.getBoolean("active")?"active":"resolved").append("\n");}new android.app.AlertDialog.Builder(this).setTitle(key).setMessage(text.toString()).setPositiveButton("Close",null).show();}catch(Exception e){status.setText("Invalid history");}});});
   JSONArray deliveries=state.getJSONArray("deliveries");for(int n=0;n<deliveries.length();n++){JSONObject d=deliveries.getJSONObject(n);if(key.equals(d.getString("key"))&&!d.isNull("error"))label(d.getString("destination")+": "+d.getString("status")+" · "+d.getString("error"));}
  }
  JSONArray silences=state.getJSONArray("silences");for(int i=0;i<silences.length();i++){JSONObject s=silences.getJSONObject(i);if(s.getLong("until")<=state.getLong("now"))continue;label("Silenced: "+s.optString("key",s.optJSONObject("labels").toString()));action("End silence",()->{JSONObject v=new JSONObject(s.toString());v.put("version",s.getLong("version")+1).put("until",0);AlertApi.request(this,port,"silence_save",new JSONObject().put("expected",s.getLong("version")).put("silence",v));});}
  JSONArray errors=state.getJSONArray("evaluations");for(int i=0;i<errors.length();i++){JSONObject e=errors.getJSONObject(i);if(!e.isNull("error"))label(e.getString("id")+": "+e.getString("error"));}
 }
 final Runnable tick=new Runnable(){public void run(){if(foreground){refresh();main.postDelayed(this,3000);}}};
 @Override public void onStart(){super.onStart();foreground=true;main.post(tick);}
 @Override public void onStop(){foreground=false;main.removeCallbacks(tick);super.onStop();}
 @Override public void onDestroy(){io.shutdownNow();super.onDestroy();}
}
