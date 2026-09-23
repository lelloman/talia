package com.lelloman.talia.dashboard;
import android.app.Activity;import android.view.*;import android.widget.*;import org.json.*;
/** Debug-only entry, invoked by the qualification harness on the UI thread. */
final class RendererChecks {
 static JSONObject run(Activity activity,LinearLayout parent,JSONArray scenes)throws Exception{
  LinearLayout root=new LinearLayout(activity);root.setOrientation(1);parent.addView(root);
  try{
   NativeRenderer renderer=new NativeRenderer(activity,root,(a,t,v)->{});renderer.render(scenes.getJSONObject(0));measure(root);
   android.widget.Switch a=null,b=null;TextView hidden=null,collapsed=null,longText=null;
   for(String id:renderer.cache.keySet()){
    View v=renderer.cache.get(id);if(v instanceof android.widget.Switch){if(((android.widget.Switch)v).getText().toString().equals("A"))a=(android.widget.Switch)v;else b=(android.widget.Switch)v;}
    if(id.endsWith("/hidden"))hidden=(TextView)v;if(id.endsWith("/collapsed"))collapsed=(TextView)v;if(id.endsWith("/long"))longText=(TextView)v;
   }
   require(a!=null&&b!=null,"native switches");a.setFocusableInTouchMode(true);require(a.requestFocus(),"initial focus");View original=a;int oldId=a.getId();
   renderer.render(scenes.getJSONObject(1));measure(root);
   require(renderer.cache.get((String)original.getTag())==original&&original.getId()==oldId,"keyed identity");require(original.hasFocus(),"focus preserved");
   require(hidden.getVisibility()==View.INVISIBLE&&hidden.getMeasuredHeight()>0,"hidden retains space");require(collapsed.getVisibility()==View.GONE,"collapsed removes space");
   require(longText.getLineCount()>1,"long text wraps");
   View one=null,two=null;for(String id:renderer.cache.keySet()){if(id.endsWith("/one"))one=renderer.cache.get(id);if(id.endsWith("/two"))two=renderer.cache.get(id);}require(one!=null&&two!=null&&one.getWidth()>0&&Math.abs(one.getWidth()-two.getWidth())<=1&&one.getLeft()!=two.getLeft(),"grid equal columns");
   JSONObject result=new JSONObject().put("passed",true).put("checks",new JSONArray(new String[]{"native keyed identity", "focus survives reorder", "hidden retains space", "collapsed removes space", "long text wraps", "native switch accessibility", "equal grid columns"}));
   require(a.getContentDescription().equals("A"),"accessible label");
   JSONObject metric=new JSONObject("{\"type\":\"Text\",\"id\":\"metric\",\"props\":{\"text\":\"24%\",\"variant\":\"metric\",\"tone\":\"warning\"},\"children\":[]}");
   JSONObject card=new JSONObject().put("type","Column").put("id","card").put("props",new JSONObject().put("surface","card")).put("children",new JSONArray().put(metric));
   renderer.render(card);TextView value=(TextView)renderer.cache.get("metric");
   require(renderer.cache.get("card").getBackground() instanceof android.graphics.drawable.GradientDrawable,"card surface");
   require(Math.abs(value.getTextSize()-android.util.TypedValue.applyDimension(android.util.TypedValue.COMPLEX_UNIT_SP,36,activity.getResources().getDisplayMetrics()))<1,"metric typography");
   metric.getJSONObject("props").put("variant","heading");renderer.render(card);require(value.isAccessibilityHeading(),"heading accessibility");
   metric.getJSONObject("props").remove("variant");card.getJSONObject("props").remove("surface");renderer.render(card);
   require(!value.isAccessibilityHeading()&&renderer.cache.get("card").getBackground()==null,"presentation reset");
   result.getJSONArray("checks").put("semantic presentation and reset");return result;
  }finally{parent.removeView(root);}
 }
 static void measure(View root){root.measure(View.MeasureSpec.makeMeasureSpec(600,View.MeasureSpec.EXACTLY),View.MeasureSpec.makeMeasureSpec(1600,View.MeasureSpec.AT_MOST));root.layout(0,0,600,root.getMeasuredHeight());}
 static void require(boolean value,String name){if(!value)throw new IllegalStateException(name);}
}
