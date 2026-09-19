// Trusted DOM renderer. Only validated resolved nodes enter this module.
export class Renderer {
 constructor(root,dispatch){this.root=root;this.dispatch=dispatch;this.cache=new Map();this.nodes=new Map();}
 render(tree,scale=1){
  const active=document.activeElement;this.nodes.clear();
  const length=v=>v==='fill'?'100%':v==='auto'?'auto':TaliaUI.px(v,scale)+'px';
  const build=n=>{
   this.nodes.set(n.id,n);let el=this.cache.get(n.id);
   if(!el){
    el=document.createElement(({Text:'p',Status:'p',Button:'button',Slider:'label',Switch:'label',Chart:'figure'})[n.type]||'div');el.dataset.nodeId=n.id;el.dataset.type=n.type;
    if(n.type==='Slider'||n.type==='Switch'){
     const caption=document.createElement('span'),input=document.createElement('input');input.type=n.type==='Slider'?'range':'checkbox';el.append(caption,input);
     input.addEventListener('change',()=>this.emit(n.id,n.type==='Slider'?Number(input.value):input.checked));
    }
    if(n.type==='Button')el.addEventListener('click',()=>this.emit(n.id,null));
    this.cache.set(n.id,el);
   }
   const p=n.props;el.hidden=p.visibility==='collapsed';el.style.visibility=p.visibility==='hidden'?'hidden':'visible';
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
    if(n.type==='Status')el.setAttribute('role','status');
    if(n.type==='Button')el.disabled=p.enabled===false;
    if(p.label)el.setAttribute('aria-label',p.label);else el.removeAttribute('aria-label');
   }else if(n.type==='Slider'||n.type==='Switch'){
    el.firstChild.textContent=p.label;const input=el.lastChild;input.setAttribute('aria-label',p.label);input.disabled=p.enabled===false;
    if(n.type==='Slider'){input.min=p.min;input.max=p.max;input.step=p.step??1;input.value=p.value;}else{input.checked=p.value;input.setAttribute('role','switch');}
   }else if(n.type==='Chart'){
    const signature=JSON.stringify([p.values,p.label]);
    if(el.dataset.chart!==signature){
     el.dataset.chart=signature;el.replaceChildren();const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 300 100');svg.setAttribute('preserveAspectRatio','none');svg.setAttribute('aria-hidden','true');
     const line=document.createElementNS(svg.namespaceURI,'polyline'),min=Math.min(0,...p.values),max=Math.max(1,...p.values),span=max-min;
     line.setAttribute('points',p.values.map((v,i)=>`${i*300/Math.max(1,p.values.length-1)},${95-(v-min)/span*90}`).join(' '));line.setAttribute('fill','none');line.setAttribute('stroke','#2563eb');line.setAttribute('stroke-width','2');svg.append(line);
     const caption=document.createElement('figcaption');caption.textContent=p.label+': '+(p.values.length?p.values.join(', '):'No samples');el.append(svg,caption);el.setAttribute('role','img');el.setAttribute('aria-label',caption.textContent);
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
  this.dispatch(handler.action,{target:id,value});
 }
}
