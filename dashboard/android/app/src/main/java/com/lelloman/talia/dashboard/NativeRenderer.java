package com.lelloman.talia.dashboard;
import android.content.Context;
import android.graphics.*;
import android.view.*;
import android.widget.*;
import org.json.*;
import java.util.*;

/** Keyed native views; ViewModels never receive Android objects. */
final class NativeRenderer {
 interface Events {void dispatch(String action,String target,Object value);}
 final Context context;final LinearLayout root;final Events events;
 final Map<String,View> cache=new HashMap<>();final Map<String,JSONObject> nodes=new HashMap<>();
 boolean updating;
 NativeRenderer(Context c,LinearLayout root,Events events){this.context=c;this.root=root;this.events=events;}
 int pixels(String v){if(v.equals("fill"))return -1;if(v.equals("auto"))return -2;float n=Float.parseFloat(v.substring(0,v.length()-2));return Math.round(n*(v.endsWith("dp")?context.getResources().getDisplayMetrics().density:1));}
 void render(JSONObject tree)throws JSONException{
  updating=true;View focused=root.findFocus();nodes.clear();
  try{View view=build(tree);if(root.getChildCount()!=1||root.getChildAt(0)!=view){detach(view);root.removeAllViews();root.addView(view,new LinearLayout.LayoutParams(-1,-2));}
   cache.keySet().retainAll(nodes.keySet());if(focused!=null&&focused.isAttachedToWindow())focused.requestFocus();
  }finally{updating=false;}
 }
 void emit(String id,Object value){
  if(updating)return;
  try{JSONObject n=nodes.get(id);if(n==null)return;JSONObject p=n.getJSONObject("props");if(!p.optBoolean("enabled",true)||!p.optString("visibility","visible").equals("visible"))return;
   String type=n.getString("type"),event=type.equals("Button")?"onClick":"onChange";
   if(type.equals("Slider")&&(!(value instanceof Number)||((Number)value).doubleValue()<p.getDouble("min")||((Number)value).doubleValue()>p.getDouble("max")))throw new IllegalArgumentException("slider range");
   if(type.equals("Switch")&&!(value instanceof Boolean))throw new IllegalArgumentException("switch value");
   events.dispatch(p.getJSONObject(event).getString("action"),id,value);
  }catch(JSONException e){throw new IllegalArgumentException(e);}
 }
 static void detach(View v){if(v.getParent() instanceof ViewGroup)((ViewGroup)v.getParent()).removeView(v);}
 View build(JSONObject n)throws JSONException{
  String id=n.getString("id"),type=n.getString("type");JSONObject p=n.getJSONObject("props");nodes.put(id,n);View v=cache.get(id);
  if(v==null){switch(type){
   case "Text":case "Status":v=new TextView(context);((TextView)v).setTextSize(16);break;
   case "Button":v=new Button(context);((Button)v).setAllCaps(false);v.setOnClickListener(x->emit(id,JSONObject.NULL));break;
   case "Switch":v=new Switch(context);((Switch)v).setOnCheckedChangeListener((b,checked)->emit(id,checked));break;
   case "Slider":{
    LinearLayout box=new LinearLayout(context);box.setOrientation(1);TextView label=new TextView(context);SeekBar slider=new SeekBar(context);box.addView(label);box.addView(slider);
    slider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onStartTrackingTouch(SeekBar s){}public void onStopTrackingTouch(SeekBar s){}public void onProgressChanged(SeekBar s,int progress,boolean user){if(user){try{JSONObject props=nodes.get(id).getJSONObject("props");double value=Math.min(props.getDouble("max"),props.getDouble("min")+progress*props.optDouble("step",1));emit(id,value);}catch(JSONException e){throw new IllegalArgumentException(e);}}}});v=box;break;
   }
   case "Chart":v=new Chart(context);break;
   case "Scroll":v=new ScrollView(context);break;
   case "Grid":v=new GridLayout(context);break;
   default:LinearLayout box=new LinearLayout(context);box.setOrientation(type.equals("Row")?0:1);v=box;
  }cache.put(id,v);v.setTag(id);v.setId(View.generateViewId());}
  String visibility=p.optString("visibility","visible");v.setVisibility(visibility.equals("collapsed")?View.GONE:visibility.equals("hidden")?View.INVISIBLE:View.VISIBLE);
  v.setEnabled(p.optBoolean("enabled",true));int padding=pixels(p.optString("padding","0dp"));v.setPadding(padding,padding,padding,padding);
  switch(type){
   case "Text":case "Status":case "Button":((TextView)v).setText(p.get("text").toString());if(p.has("label"))v.setContentDescription(p.getString("label"));if(type.equals("Status"))v.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);break;
   case "Switch":((Switch)v).setText(p.getString("label"));v.setContentDescription(p.getString("label"));((Switch)v).setChecked(p.getBoolean("value"));break;
   case "Slider":{
    LinearLayout box=(LinearLayout)v;((TextView)box.getChildAt(0)).setText(p.getString("label"));SeekBar s=(SeekBar)box.getChildAt(1);s.setContentDescription(p.getString("label"));s.setEnabled(p.optBoolean("enabled",true));double step=p.optDouble("step",1);s.setMax((int)Math.ceil((p.getDouble("max")-p.getDouble("min"))/step));s.setProgress((int)Math.round((p.getDouble("value")-p.getDouble("min"))/step));break;
   }
   case "Chart":((Chart)v).set(p.getJSONArray("values"),p.getString("label"),p.optJSONArray("sampleLabels"));break;
   default:{
    ViewGroup group=(ViewGroup)v;JSONArray children=n.getJSONArray("children");int gap=pixels(p.optString("gap","0dp"));
    if(group instanceof GridLayout)((GridLayout)group).setColumnCount(p.optInt("columns",1));
    for(int i=0;i<children.length();i++){
     JSONObject child=children.getJSONObject(i);View cv=build(child);JSONObject cp=child.getJSONObject("props");
     int width=cp.has("width")?pixels(cp.getString("width")):(type.equals("Row")?-2:-1),height=cp.has("height")?pixels(cp.getString("height")):-2;
     if(child.getString("type").equals("Chart")&&!cp.has("height"))height=pixels("120dp");
     ViewGroup.MarginLayoutParams lp;
     if(group instanceof GridLayout){GridLayout.LayoutParams g=new GridLayout.LayoutParams();int columns=p.optInt("columns",1);g.columnSpec=GridLayout.spec(i%columns,1f);g.rowSpec=GridLayout.spec(i/columns);g.width=0;g.height=height;g.setMargins(0,0,gap,gap);lp=g;}
     else{lp=group instanceof LinearLayout?new LinearLayout.LayoutParams(width,height):new android.widget.FrameLayout.LayoutParams(width,height);if(i>0){if(type.equals("Row"))lp.leftMargin=gap;else lp.topMargin=gap;}}
     if(group.getChildCount()<=i||group.getChildAt(i)!=cv){detach(cv);group.addView(cv,i,lp);}else cv.setLayoutParams(lp);
    }
    while(group.getChildCount()>children.length())group.removeViewAt(group.getChildCount()-1);break;
   }
  }
  return v;
 }
 static final class Chart extends View{
  final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);double[] values=new double[0];String label="";
  Chart(Context c){super(c);setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);setMinimumHeight(100);}
  void set(JSONArray data,String label,JSONArray labels)throws JSONException{this.label=label;values=new double[data.length()];StringBuilder text=new StringBuilder(label+": ");for(int i=0;i<values.length;i++){values[i]=data.isNull(i)?Double.NaN:data.getDouble(i);if(i>0)text.append(", ");text.append(labels==null?String.valueOf(values[i]):labels.getString(i));}if(values.length==0)text.append("No samples");setContentDescription(text.toString());invalidate();}
  protected void onDraw(Canvas canvas){super.onDraw(canvas);double min=0,max=1;for(double v:values){if(Double.isFinite(v)){min=Math.min(min,v);max=Math.max(max,v);}}paint.setColor(Color.rgb(37,99,235));paint.setStrokeWidth(3);for(int i=1;i<values.length;i++)if(Double.isFinite(values[i-1])&&Double.isFinite(values[i]))canvas.drawLine((i-1)*getWidth()/(float)Math.max(1,values.length-1),(float)((getHeight()-24)*(1-(values[i-1]-min)/(max-min))),i*getWidth()/(float)Math.max(1,values.length-1),(float)((getHeight()-24)*(1-(values[i]-min)/(max-min))),paint);paint.setTextSize(24);canvas.drawText(getContentDescription().toString(),0,getHeight()-2,paint);}
 }
}
