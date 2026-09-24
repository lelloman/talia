// Trusted DOM renderer. Only validated resolved nodes enter this module.
export class Renderer {
 constructor(root,dispatch){this.root=root;this.dispatch=dispatch;this.cache=new Map();this.nodes=new Map();this.pending=new Map();this.eventSequence=0;}
 render(tree,scale=1){
  const active=document.activeElement;this.nodes.clear();
  const length=v=>v==='fill'?'100%':v==='auto'?'auto':TaliaUI.px(v,scale)+'px';
  const build=n=>{
   this.nodes.set(n.id,n);let el=this.cache.get(n.id);
   if(!el){
    el=document.createElement(({Text:'p',Status:'p',Button:'button',Slider:'label',Switch:'label',Chart:'figure'})[n.type]||'div');el.dataset.nodeId=n.id;el.dataset.type=n.type;
    if(n.type==='Slider'||n.type==='Switch'){
     const caption=document.createElement('span'),input=document.createElement('input');input.type=n.type==='Slider'?'range':'checkbox';el.append(caption,input);
     input.addEventListener(n.type==='Slider'?'input':'change',()=>this.emit(n.id,n.type==='Slider'?Number(input.value):input.checked));
    }
    if(n.type==='Button')el.addEventListener('click',()=>this.emit(n.id,null));
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
   }else if(n.type==='Text'||n.type==='Status'||n.type==='Button'){
    if(el.textContent!==String(p.text))el.textContent=String(p.text);
    if(p.variant==='heading'){el.setAttribute('role','heading');el.setAttribute('aria-level','2');}else{el.removeAttribute('aria-level');if(n.type==='Status')el.setAttribute('role','status');else el.removeAttribute('role');}
    if(n.type==='Button')el.disabled=p.enabled===false;
    if(p.label)el.setAttribute('aria-label',p.label);else el.removeAttribute('aria-label');
   }else if(n.type==='Slider'||n.type==='Switch'){
    el.firstChild.textContent=p.label;const input=el.lastChild;input.setAttribute('aria-label',p.label);input.disabled=p.enabled===false;
    if(n.type==='Slider'){input.min=p.min;input.max=p.max;input.step=p.step??1;if(!this.pending.has(n.id))input.value=p.value;}else{if(!this.pending.has(n.id))input.checked=p.value;input.setAttribute('role','switch');}
   }else if(n.type==='Chart'){
    const signature=JSON.stringify([p.values,p.label,p.sampleLabels,p.min,p.max,p.unit,p.threshold,p.startLabel,p.endLabel]);
    if(el.dataset.chart!==signature){
     el.dataset.chart=signature;el.replaceChildren();const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 360 130');svg.setAttribute('preserveAspectRatio','none');svg.setAttribute('aria-hidden','true');
     const finite=p.values.filter(Number.isFinite),fixed=p.min!==undefined,min=fixed?p.min:Math.min(0,...finite),max=fixed?p.max:Math.max(1,...finite),span=max-min,x0=fixed?39:4,x1=354,y0=8,y1=106;
     const make=(name,attrs)=>{const node=document.createElementNS(svg.namespaceURI,name);for(const [key,value]of Object.entries(attrs))node.setAttribute(key,String(value));svg.append(node);return node;};
     const y=value=>y1-(Math.min(max,Math.max(min,value))-min)/span*(y1-y0);
     if(fixed){
      for(let i=0;i<=4;i++){const value=min+span*i/4,at=y(value);make('line',{x1:x0,y1:at,x2:x1,y2:at,stroke:'var(--ld-border-subtle, #d1d5db)','stroke-width':1,'vector-effect':'non-scaling-stroke'});const label=make('text',{x:x0-6,y:at+3,'text-anchor':'end',fill:'var(--ld-text-secondary, #6b7280)','font-size':10});label.textContent=Number.isInteger(value)?value+(p.unit||''):value.toFixed(1)+(p.unit||'');}
      if(p.startLabel){const left=make('text',{x:x0,y:124,fill:'var(--ld-text-secondary, #6b7280)','font-size':10});left.textContent=p.startLabel;}
      if(p.endLabel){const right=make('text',{x:x1,y:124,'text-anchor':'end',fill:'var(--ld-text-secondary, #6b7280)','font-size':10});right.textContent=p.endLabel;}
     }
     if(Number.isFinite(p.threshold)&&p.threshold>=min&&p.threshold<=max){const at=y(p.threshold);make('line',{x1:x0,y1:at,x2:x1,y2:at,stroke:'var(--ld-warning, #b45309)','stroke-width':1.5,'stroke-dasharray':'5 4','vector-effect':'non-scaling-stroke'});const label=make('text',{x:x1-4,y:Math.max(13,at-4),'text-anchor':'end',fill:'var(--ld-warning, #b45309)','font-size':10});label.textContent=p.threshold+(p.unit||'')+' high';}
     let segment=[];const flush=()=>{if(!segment.length)return;make('polyline',{points:segment.join(' '),fill:'none',stroke:'var(--ld-primary, #2563eb)','stroke-width':2,'vector-effect':'non-scaling-stroke'});segment=[];};
     p.values.forEach((v,i)=>{if(v===null){flush();return;}segment.push(`${x0+i*(x1-x0)/Math.max(1,p.values.length-1)},${y(v)}`);});flush();
     const caption=document.createElement('figcaption');caption.textContent=p.label+(finite.length?'':' · No samples');el.append(svg,caption);el.setAttribute('role','img');el.setAttribute('aria-label',p.label+(fixed?`; range ${min} to ${max}${p.unit||''}`:'')+(Number.isFinite(p.threshold)?`; high reference ${p.threshold}${p.unit||''}`:'')+(p.startLabel&&p.endLabel?`; from ${p.startLabel} to ${p.endLabel}`:'')+': '+(finite.length?(p.sampleLabels||p.values).join(', '):'No samples'));
    }
   }
   return el;
  };
  const child=build(tree);if(this.root.firstChild!==child)this.root.replaceChildren(child);
  for(const [id,el]of this.cache)if(!this.nodes.has(id)){el.remove();this.cache.delete(id);}
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
