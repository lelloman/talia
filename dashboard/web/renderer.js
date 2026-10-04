// Trusted DOM renderer. Only validated resolved nodes enter this module.
import {renderSegmentedControl} from './dist/chrome.js';
function paintChart(canvas,p){
 const rect=canvas.getBoundingClientRect();if(rect.width<1||rect.height<1)return;
 const dpr=window.devicePixelRatio||1,width=rect.width,height=rect.height;
 const backingWidth=Math.max(1,Math.round(width*dpr)),backingHeight=Math.max(1,Math.round(height*dpr));
 if(canvas.width!==backingWidth||canvas.height!==backingHeight){canvas.width=backingWidth;canvas.height=backingHeight;}
 const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,width,height);
 const css=getComputedStyle(canvas),color=(name,fallback)=>css.getPropertyValue(name).trim()||fallback;
 // Normalizes a theme color through the canvas so translucent fills can reuse it.
 const alpha=(value,a)=>{ctx.fillStyle='#000';ctx.fillStyle=value;const hex=/^#([0-9a-f]{6})$/i.exec(ctx.fillStyle);return hex?`rgba(${parseInt(hex[1].slice(0,2),16)},${parseInt(hex[1].slice(2,4),16)},${parseInt(hex[1].slice(4),16)},${a})`:null;};
 const finite=p.values.filter(Number.isFinite),fixed=p.min!==undefined,min=fixed?p.min:Math.min(0,...finite),max=fixed?p.max:Math.max(1,...finite),span=max-min;
 ctx.font='11px '+(css.fontFamily||'system-ui, sans-serif');const x0=fixed?Math.max(37,ctx.measureText(String(max)+(p.unit||'')).width+8):4,x1=width-7,y0=9,y1=height-(fixed?25:5);
 if(x1<=x0||y1<=y0)return;
 const y=value=>y1-(Math.min(max,Math.max(min,value))-min)/span*(y1-y0),x=i=>x0+i*(x1-x0)/Math.max(1,p.values.length-1);
 const primary=color('--ld-primary','#2563eb'),warning=color('--ld-warning','#b45309'),muted=color('--ld-text-secondary','#6b7280');
 if(fixed){
  ctx.strokeStyle=color('--ld-border-subtle','#d1d5db');ctx.fillStyle=muted;ctx.lineWidth=1;ctx.textBaseline='middle';
  for(let i=0;i<=4;i++){const value=min+span*i/4,at=Math.round(y(value))+.5;ctx.setLineDash(i?[2,3]:[]);ctx.beginPath();ctx.moveTo(x0,at);ctx.lineTo(x1,at);ctx.stroke();ctx.textAlign='right';ctx.fillText((Number.isInteger(value)?String(value):value.toFixed(1))+(p.unit||''),x0-6,at);}
  ctx.setLineDash([]);ctx.textBaseline='alphabetic';if(p.startLabel){ctx.textAlign='left';ctx.fillText(p.startLabel,x0,height-4);}if(p.endLabel){ctx.textAlign='right';ctx.fillText(p.endLabel,x1,height-4);}
 }
 // Contiguous runs of samples; missing samples stay visible gaps.
 const runs=[];let run=null;
 p.values.forEach((value,i)=>{if(!Number.isFinite(value)){run=null;return;}if(!run)runs.push(run=[]);run.push([x(i),y(value)]);});
 const trace=()=>{ctx.beginPath();for(const r of runs)r.forEach(([px,py],i)=>i?ctx.lineTo(px,py):ctx.moveTo(px,py));};
 const top=alpha(primary,.28),bottom=alpha(primary,0);
 if(top){const fill=ctx.createLinearGradient(0,y0,0,y1);fill.addColorStop(0,top);fill.addColorStop(1,bottom);ctx.fillStyle=fill;}else{ctx.fillStyle=primary;ctx.globalAlpha=.12;}
 for(const r of runs){if(r.length<2)continue;ctx.beginPath();ctx.moveTo(r[0][0],y1);r.forEach(([px,py])=>ctx.lineTo(px,py));ctx.lineTo(r[r.length-1][0],y1);ctx.closePath();ctx.fill();}
 ctx.globalAlpha=1;ctx.lineWidth=1.75;ctx.lineJoin='round';ctx.lineCap='round';ctx.strokeStyle=primary;trace();ctx.stroke();
 const thresholdShown=Number.isFinite(p.threshold)&&p.threshold>=min&&p.threshold<=max;
 if(thresholdShown){
  const at=y(p.threshold);
  ctx.save();ctx.beginPath();ctx.rect(0,0,width,at);ctx.clip();ctx.strokeStyle=warning;trace();ctx.stroke();ctx.restore();
  ctx.strokeStyle=warning;ctx.fillStyle=warning;ctx.lineWidth=1.25;ctx.setLineDash([5,4]);ctx.beginPath();ctx.moveTo(x0,at);ctx.lineTo(x1,at);ctx.stroke();ctx.setLineDash([]);ctx.textAlign='right';ctx.textBaseline='alphabetic';ctx.fillText(p.threshold+(p.unit||'')+' high',x1-4,Math.max(12,at-4));
 }
 const last=runs.at(-1)?.at(-1);
 if(last&&last[0]>=x1-1){const value=finite.at(-1);ctx.beginPath();ctx.arc(last[0],last[1],3.5,0,Math.PI*2);ctx.fillStyle=thresholdShown&&value>p.threshold?warning:primary;ctx.fill();ctx.lineWidth=2;ctx.strokeStyle=color('--ld-surface','#fff');ctx.stroke();}
}
export class Renderer {
 constructor(root,dispatch){this.root=root;this.dispatch=dispatch;this.cache=new Map();this.nodes=new Map();this.pending=new Map();this.eventSequence=0;this.chartResize=new ResizeObserver(entries=>{for(const entry of entries){const canvas=entry.target;if(canvas.isConnected&&canvas.chartProps)paintChart(canvas,canvas.chartProps);}});const theme=root.closest('[data-lello-theme]');if(theme){this.chartTheme=new MutationObserver(()=>{for(const el of this.cache.values()){const canvas=el.querySelector('canvas');if(canvas?.isConnected&&canvas.chartProps)paintChart(canvas,canvas.chartProps);}});this.chartTheme.observe(theme,{attributes:true,attributeFilter:['data-lello-theme']});}}
 render(tree,scale=1){
  const active=document.activeElement,redraw=[];this.nodes.clear();
  const length=v=>v==='fill'?'100%':v==='auto'?'auto':TaliaUI.px(v,scale)+'px';
  const build=n=>{
   this.nodes.set(n.id,n);let el=this.cache.get(n.id);
   if(!el){
    el=document.createElement(({Text:'p',Status:'p',Button:'button',Slider:'label',Switch:'label',Chart:'figure'})[n.type]||'div');el.dataset.nodeId=n.id;el.dataset.type=n.type;
    if(n.type==='Slider'||n.type==='Switch'){
     const caption=document.createElement('span'),input=document.createElement('input');input.type=n.type==='Slider'?'range':'checkbox';el.append(caption,input);
     input.addEventListener(n.type==='Slider'?'input':'change',()=>this.emit(n.id,n.type==='Slider'?Number(input.value):input.checked));
    }
    if(n.type==='Button'){el.className='lv-button lv-button--neutral';el.addEventListener('click',()=>this.emit(n.id,null));}
    if(n.type==='Meter'){el.setAttribute('role','meter');el.append(document.createElement('span'));}
    this.cache.set(n.id,el);
   }
   const p=n.props;el.dataset.surface=p.surface||'plain';el.dataset.variant=p.variant||'body';el.dataset.tone=p.tone||'neutral';el.hidden=p.visibility==='collapsed';el.style.visibility=p.visibility==='hidden'?'hidden':'';
   for(const k of ['width','height','gap','padding'])el.style[k]=p[k]!==undefined?length(p[k]):'';
   if(['Row','Column','Grid','Scroll'].includes(n.type)){
    el.style.display=n.type==='Grid'?'grid':'flex';el.style.flexDirection=n.type==='Row'?'row':'column';
    el.style.gridTemplateColumns=n.type==='Grid'?`repeat(${p.columns||1},minmax(0,1fr))`:'';
    el.style.overflowY=n.type==='Scroll'?'auto':'';
    const children=n.children.map(build);
    children.forEach((child,i)=>{if(el.children[i]!==child)el.insertBefore(child,el.children[i]||null);});
    while(el.children.length>children.length)el.lastElementChild.remove();
   }else if(n.type==='SegmentedControl'){
    for(const child of n.children)this.nodes.set(child.id,child);
    renderSegmentedControl(el,{name:'talia-'+n.id,label:p.label,modelValue:n.children.find(c=>c.props.selected)?.id||'',options:n.children.map(c=>({value:c.id,label:c.props.text,accessibleLabel:c.props.label,disabled:c.props.enabled===false})),'onUpdate:modelValue':id=>this.emit(id,null)});
   }else if(n.type==='Text'||n.type==='Status'||n.type==='Button'){
    if(el.textContent!==String(p.text))el.textContent=String(p.text);
    if(p.variant==='heading'){el.setAttribute('role','heading');el.setAttribute('aria-level','2');}else{el.removeAttribute('aria-level');if(n.type==='Status')el.setAttribute('role','status');else el.removeAttribute('role');}
    if(n.type==='Button'){el.disabled=p.enabled===false;if(p.selected===undefined)el.removeAttribute('aria-pressed');else el.setAttribute('aria-pressed',String(p.selected));}
    if(p.label)el.setAttribute('aria-label',p.label);else el.removeAttribute('aria-label');
   }else if(n.type==='Slider'||n.type==='Switch'){
    el.firstChild.textContent=p.label;const input=el.lastChild;input.setAttribute('aria-label',p.label);input.disabled=p.enabled===false;
    if(n.type==='Slider'){input.min=p.min;input.max=p.max;input.step=p.step??1;if(!this.pending.has(n.id))input.value=p.value;}else{if(!this.pending.has(n.id))input.checked=p.value;input.setAttribute('role','switch');}
   }else if(n.type==='Meter'){
    const ratio=p.unavailable!==undefined?0:(p.value-p.min)/(p.max-p.min);el.firstChild.style.width=(ratio*100).toFixed(2)+'%';
    el.setAttribute('aria-label',p.label);el.setAttribute('aria-valuemin',p.min);el.setAttribute('aria-valuemax',p.max);
    if(p.unavailable!==undefined){el.removeAttribute('aria-valuenow');el.setAttribute('aria-valuetext','Unavailable');}else{el.setAttribute('aria-valuenow',p.value);el.removeAttribute('aria-valuetext');}
   }else if(n.type==='Chart'){
    const signature=JSON.stringify([p.values,p.label,p.sampleLabels,p.min,p.max,p.unit,p.threshold,p.startLabel,p.endLabel]);
    if(el.dataset.chart!==signature){
     el.dataset.chart=signature;let canvas=el.querySelector('canvas');if(!canvas){canvas=document.createElement('canvas');canvas.setAttribute('aria-hidden','true');el.append(canvas,document.createElement('figcaption'));this.chartResize.observe(canvas);}
     canvas.chartProps=p;redraw.push(canvas);el.lastElementChild.textContent=p.label+(p.values.some(Number.isFinite)?'':' · No samples');el.setAttribute('role','img');el.setAttribute('aria-label',p.label+(p.min!==undefined?`; range ${p.min} to ${p.max}${p.unit||''}`:'')+(Number.isFinite(p.threshold)?`; high reference ${p.threshold}${p.unit||''}`:'')+(p.startLabel&&p.endLabel?`; from ${p.startLabel} to ${p.endLabel}`:'')+': '+(p.values.some(Number.isFinite)?(p.sampleLabels||p.values).join(', '):'No samples'));
    }
   }
   return el;
  };
  const child=build(tree);if(this.root.firstChild!==child)this.root.replaceChildren(child);
  for(const [id,el]of this.cache)if(!this.nodes.has(id)){if(el.dataset.type==='SegmentedControl')renderSegmentedControl(el,null);const canvas=el.querySelector('canvas');if(canvas)this.chartResize.unobserve(canvas);el.remove();this.cache.delete(id);}
  for(const canvas of redraw)if(canvas.isConnected)paintChart(canvas,canvas.chartProps);
  if(active?.isConnected&&document.activeElement!==active)active.focus({preventScroll:true});
 }
 emit(id,value){
  const n=this.nodes.get(id);if(!n||n.props.enabled===false||['hidden','collapsed'].includes(n.props.visibility))return;
  const event=n.type==='Button'?'onClick':'onChange';const handler=n.props[event];if(!handler)throw Error('unknown event');
  if(n.type==='Slider'&&(!Number.isFinite(value)||value<n.props.min||value>n.props.max))throw Error('slider range');
  if(n.type==='Switch'&&typeof value!=='boolean')throw Error('switch value');
  const sequence=++this.eventSequence;this.pending.set(id,sequence);
  const done=()=>{if(this.pending.get(id)===sequence)this.pending.delete(id);};
  Promise.resolve(this.dispatch(handler.action,{target:id,value})).then(done,done);
 }
}
