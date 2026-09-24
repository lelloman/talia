package com.lelloman.talia.dashboard;
import android.content.Context;
import android.graphics.*;
import android.graphics.drawable.GradientDrawable;
import android.content.res.Configuration;
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
  boolean dark=(context.getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK)==Configuration.UI_MODE_NIGHT_YES;
  if(type.equals("Column")||type.equals("Row")||type.equals("Grid")){
   if(p.optString("surface").equals("card")){GradientDrawable bg=new GradientDrawable();bg.setColor(Color.parseColor(dark?"#111827":"#ffffff"));bg.setCornerRadius(pixels("12dp"));bg.setStroke(pixels("1dp"),Color.parseColor(dark?"#374151":"#e5e7eb"));v.setBackground(bg);}else v.setBackground(null);
  }
  if(type.equals("Text")||type.equals("Status")){
   TextView text=(TextView)v;String variant=p.optString("variant","body"),tone=p.optString("tone","neutral");
   text.setTextSize(variant.equals("metric")?36:variant.equals("heading")?18:variant.equals("caption")?12:16);
   text.setTypeface(null,variant.equals("metric")||variant.equals("heading")?Typeface.BOLD:Typeface.NORMAL);
   if(android.os.Build.VERSION.SDK_INT>=28)v.setAccessibilityHeading(variant.equals("heading"));
   String color=dark?"#f9fafb":"#111827";
   if(tone.equals("muted"))color=dark?"#e5e7eb":"#374151";
   if(tone.equals("success"))color=dark?"#86efac":"#166534";
   if(tone.equals("warning"))color=dark?"#fcd34d":"#92400e";
   if(tone.equals("error"))color=dark?"#fca5a5":"#b91c1c";
   text.setTextColor(Color.parseColor(color));
  }
  switch(type){
   case "Text":case "Status":((TextView)v).setText(p.get("text").toString());if(p.has("label"))v.setContentDescription(p.getString("label"));if(type.equals("Status"))v.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);break;
   case "Button":{
    Button button=(Button)v;button.setText(p.getString("text"));
    if(p.has("selected")){
     boolean selected=p.getBoolean("selected");button.setSelected(selected);button.setTypeface(null,selected?Typeface.BOLD:Typeface.NORMAL);
     GradientDrawable background=new GradientDrawable();background.setColor(Color.parseColor(selected?(dark?"#1e3a8a":"#dbeafe"):(dark?"#111827":"#ffffff")));background.setCornerRadius(pixels("24dp"));background.setStroke(pixels("1dp"),Color.parseColor(dark?"#4b5563":"#cbd5e1"));button.setBackground(background);
     button.setTextColor(Color.parseColor(dark?"#f9fafb":"#1e3a8a"));button.setTextSize(14);button.setPadding(pixels("12dp"),0,pixels("12dp"),0);button.setMinimumHeight(pixels("40dp"));
     String label=p.optString("label",p.getString("text"));button.setContentDescription(android.os.Build.VERSION.SDK_INT>=30?label:label+(selected?", selected":""));if(android.os.Build.VERSION.SDK_INT>=30)button.setStateDescription(selected?"Selected":"Not selected");
    }else if(p.has("label"))button.setContentDescription(p.getString("label"));
    break;
   }
   case "Switch":((Switch)v).setText(p.getString("label"));v.setContentDescription(p.getString("label"));((Switch)v).setChecked(p.getBoolean("value"));break;
   case "Slider":{
    LinearLayout box=(LinearLayout)v;((TextView)box.getChildAt(0)).setText(p.getString("label"));SeekBar s=(SeekBar)box.getChildAt(1);s.setContentDescription(p.getString("label"));s.setEnabled(p.optBoolean("enabled",true));double step=p.optDouble("step",1);s.setMax((int)Math.ceil((p.getDouble("max")-p.getDouble("min"))/step));s.setProgress((int)Math.round((p.getDouble("value")-p.getDouble("min"))/step));break;
   }
   case "Chart":((Chart)v).set(p.getJSONArray("values"),p.getString("label"),p.optJSONArray("sampleLabels"),p);break;
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
  final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);double[] values=new double[0];String label="",unit="",startLabel="",endLabel="";double lower=0,upper=1,threshold=Double.NaN;boolean fixed;
  Chart(Context c){super(c);setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);setMinimumHeight(100);}
  void set(JSONArray data,String label,JSONArray labels,JSONObject props)throws JSONException{this.label=label;unit=props.optString("unit","");startLabel=props.optString("startLabel","");endLabel=props.optString("endLabel","");fixed=props.has("min")&&props.has("max");lower=props.optDouble("min",0);upper=props.optDouble("max",1);threshold=props.optDouble("threshold",Double.NaN);values=new double[data.length()];StringBuilder text=new StringBuilder(label);if(fixed)text.append("; range ").append(lower).append(" to ").append(upper).append(unit);if(Double.isFinite(threshold))text.append("; high reference ").append(threshold).append(unit);if(!startLabel.isEmpty()&&!endLabel.isEmpty())text.append("; from ").append(startLabel).append(" to ").append(endLabel);text.append(": ");int finite=0;for(int i=0;i<values.length;i++){values[i]=data.isNull(i)?Double.NaN:data.getDouble(i);if(Double.isFinite(values[i]))finite++;if(i>0)text.append(", ");text.append(labels==null?String.valueOf(values[i]):labels.getString(i));}if(finite==0)text.append("No samples");setContentDescription(text.toString());invalidate();}
  protected void onDraw(Canvas canvas){super.onDraw(canvas);float scale=getResources().getDisplayMetrics().density,x0=fixed?40*scale:4*scale,x1=getWidth()-6*scale,y0=8*scale,y1=getHeight()-(fixed?38:24)*scale;double min=fixed?lower:0,max=fixed?upper:1;for(double v:values)if(!fixed&&Double.isFinite(v)){min=Math.min(min,v);max=Math.max(max,v);}if(y1<=y0||x1<=x0)return;
   boolean dark=(getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK)==Configuration.UI_MODE_NIGHT_YES;
   if(fixed){paint.setTextSize(10*scale);paint.setStrokeWidth(scale);paint.setColor(Color.parseColor(dark?"#9ca3af":"#6b7280"));for(int i=0;i<=4;i++){double value=min+(max-min)*i/4;float y=(float)(y1-(value-min)/(max-min)*(y1-y0));canvas.drawLine(x0,y,x1,y,paint);String tick=(value==Math.rint(value)?String.valueOf((long)value):String.format(java.util.Locale.US,"%.1f",value))+unit;canvas.drawText(tick,2*scale,y+3*scale,paint);}if(!startLabel.isEmpty())canvas.drawText(startLabel,x0,getHeight()-25*scale,paint);if(!endLabel.isEmpty())canvas.drawText(endLabel,x1-paint.measureText(endLabel),getHeight()-25*scale,paint);}
   if(Double.isFinite(threshold)&&threshold>=min&&threshold<=max){paint.setColor(Color.parseColor(dark?"#fcd34d":"#b45309"));paint.setStrokeWidth(1.5f*scale);paint.setPathEffect(new DashPathEffect(new float[]{5*scale,4*scale},0));float y=(float)(y1-(threshold-min)/(max-min)*(y1-y0));canvas.drawLine(x0,y,x1,y,paint);paint.setPathEffect(null);paint.setTextSize(10*scale);String text=(threshold==Math.rint(threshold)?String.valueOf((long)threshold):String.valueOf(threshold))+unit+" high";canvas.drawText(text,x1-paint.measureText(text)-3*scale,Math.max(13*scale,y-4*scale),paint);}
   paint.setColor(Color.rgb(37,99,235));paint.setStrokeWidth(2*scale);for(int i=1;i<values.length;i++)if(Double.isFinite(values[i-1])&&Double.isFinite(values[i]))canvas.drawLine(x0+(i-1)*(x1-x0)/Math.max(1,values.length-1),(float)(y1-(Math.min(max,Math.max(min,values[i-1]))-min)/(max-min)*(y1-y0)),x0+i*(x1-x0)/Math.max(1,values.length-1),(float)(y1-(Math.min(max,Math.max(min,values[i]))-min)/(max-min)*(y1-y0)),paint);
   paint.setColor(Color.parseColor(dark?"#f9fafb":"#111827"));paint.setTextSize(12*scale);canvas.drawText(label,0,getHeight()-2*scale,paint);
  }
 }
}
