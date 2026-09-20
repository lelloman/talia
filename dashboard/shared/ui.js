/* Shared trusted UI compiler/resolver. Does not evaluate source as JavaScript. */
(() => {
  const dimension = /^(?:0|[0-9]+(?:\.[0-9]+)?)(dp|px)$/;
  const pathPattern = /^(state|params|item)\.[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$/;
  const forbidden = new Set(['__proto__','prototype','constructor']);
  const base = {visibility:'visibility'};
  const layout = {...base,gap:'length',padding:'length',width:'size',height:'size'};
  const schema = {
    Dashboard:{}, Screen:{}, Surface:{}, ScreenRef:{screen:'string!'},
    Column:layout, Row:layout, Grid:{...layout,columns:'positive'},
    Scroll:{...base,width:'size',height:'size'},
    Text:{...base,text:'text!',label:'string'}, Status:{...base,text:'text!',label:'string'},
    Chart:{...base,values:'numbers!',label:'string!',height:'length'},
    Button:{...base,text:'string!',enabled:'boolean',onClick:'action!'},
    Slider:{...base,value:'number!',min:'number!',max:'number!',step:'positive',label:'string!',enabled:'boolean',onChange:'action!'},
    Switch:{...base,value:'boolean!',label:'string!',enabled:'boolean',onChange:'action!'},
    If:{when:'boolean!'}, For:{items:'array!',key:'key!'},
    Use:{definition:'string!',params:'object!'}, Width:{min:'length',max:'length'}
  };
  function fail(node,message) {throw Error(`${node?.source?.line||1}:${node?.source?.column||1}: ${message}`);}
  function pathValid(path) {return pathPattern.test(path)&&!path.split('.').some(p=>forbidden.has(p));}
  function checkValue(type,v,n) {
    type=type.replace('!','');
    const ok=({string:()=>typeof v==='string',text:()=>typeof v==='string'||typeof v==='number'&&Number.isFinite(v),
      number:()=>typeof v==='number'&&Number.isFinite(v),positive:()=>typeof v==='number'&&Number.isFinite(v)&&v>0,
      boolean:()=>typeof v==='boolean',length:()=>typeof v==='string'&&dimension.test(v),
      size:()=>['fill','auto'].includes(v)||typeof v==='string'&&dimension.test(v),
      visibility:()=>['visible','hidden','collapsed'].includes(v),
      array:()=>Array.isArray(v),numbers:()=>Array.isArray(v)&&v.every(x=>x===null||typeof x==='number'&&Number.isFinite(x)),
      object:()=>v!==null&&typeof v==='object'&&!Array.isArray(v),
      action:()=>v&&typeof v.action==='string'&&/^[A-Za-z_$][\w$]*$/.test(v.action),
      key:()=>v&&typeof v.bind==='string'&&v.bind.startsWith('item.')&&!v.not
    })[type];
    if(!ok||!ok())fail(n,`expected ${type}`);
  }
  function compile(source) {
    if(typeof source!=='string'||source.length>262144)throw Error('UI source limit');
    let pos=0,count=0;
    const location=()=>({line:source.slice(0,pos).split('\n').length,column:pos-(source.lastIndexOf('\n',pos-1)+1)+1});
    const err=message=>fail({source:location()},message);
    function ws(){while(/\s/.test(source[pos]||'')&&pos<source.length)pos++;}
    function take(re,what){const m=re.exec(source.slice(pos));if(!m)err('expected '+what);pos+=m[0].length;return m[0];}
    function node(depth=0){
      if(depth>64||++count>2048)err('UI complexity limit');
      ws();const at=location();take(/^</,'<');const type=take(/^[A-Za-z]+/,'component');
      const props={};let id;
      ws();while(pos<source.length&&!source.startsWith('/>',pos)&&source[pos]!=='>'){
        const name=take(/^[A-Za-z][A-Za-z0-9]*/,'property');
        if(name in props||name==='id'&&id!==undefined)err('duplicate property '+name);
        ws();take(/^=/,'=');ws();let value;
        if(source[pos]==='"'){
          const raw=take(/^"(?:[^"\\\r\n]|\\["\\/bfnrt]|\\u[0-9a-fA-F]{4})*"/,'quoted string');value=JSON.parse(raw);
        }else{
          take(/^\{/,'{');const end=source.indexOf('}',pos);if(end<0)err('unclosed expression');
          const expr=source.slice(pos,end).trim();pos=end+1;
          if(/^actions\.[A-Za-z_$][\w$]*$/.test(expr))value={action:expr.slice(8)};
          else if(pathValid(expr.replace(/^!/,'')))value={bind:expr.replace(/^!/,''),not:expr.startsWith('!')};
          else {try{value=JSON.parse(expr);}catch{err('unsupported expression');}
            if(value!==null&&!['string','number','boolean'].includes(typeof value)||typeof value==='number'&&!Number.isFinite(value))err('only scalar literals or paths allowed');}
        }
        if(name==='id')id=value;else props[name]=value;ws();
      }
      const children=[];
      if(source.startsWith('/>',pos))pos+=2;
      else {take(/^>/,'>');ws();while(!source.startsWith('</',pos)){if(pos>=source.length)err('unclosed '+type);children.push(node(depth+1));ws();}
        take(new RegExp('^</'+type+'\\s*>'),'closing '+type);}
      return {type,id,props,children,source:at};
    }
    const root=node();ws();if(pos!==source.length)err('trailing content');
    const tree={version:1,root};validate(tree);return tree;
  }
  function validate(tree,definitions={}) {
    if(tree?.version!==1||!tree.root)throw Error('unsupported UI version');
    const ids=new Set();let count=0;
    function visit(n,depth=0,inItem=false){
      if(depth>64||++count>2048)fail(n,'UI complexity limit');
      if(!n||!Object.hasOwn(schema,n.type))fail(n,'unknown component');
      if(typeof n.id!=='string'||!/^[A-Za-z][\w-]*$/.test(n.id)||ids.has(n.id))fail(n,'missing, invalid or duplicate id');ids.add(n.id);
      if(!n.props||!Array.isArray(n.children))fail(n,'invalid node');
      const spec=schema[n.type];
      for(const [key,type] of Object.entries(spec))if(type.endsWith('!')&&!Object.hasOwn(n.props,key))fail(n,'missing '+key);
      for(const [key,v] of Object.entries(n.props)){
        if(!Object.hasOwn(spec,key))fail(n,'unknown property '+key);
        if(v&&typeof v==='object'&&Object.hasOwn(v,'bind')){
          if(!pathValid(v.bind)||typeof v.not!=='boolean'||v.not&&!spec[key].startsWith('boolean'))fail(n,'invalid binding');
          if(v.bind.startsWith('item.')&&!inItem&&!(n.type==='For'&&key==='key'))fail(n,'item outside For');
          if(spec[key].startsWith('action'))fail(n,'event must name an action');
          if(spec[key].startsWith('key'))checkValue('key',v,n);
        }else checkValue(spec[key],v,n);
      }
      const leaf=['Text','Status','Chart','Button','Slider','Switch','ScreenRef','Use'];
      if(leaf.includes(n.type)&&n.children.length)fail(n,'leaf cannot have children');
      if(['Screen','Surface','Scroll','If','For','Width'].includes(n.type)&&n.children.length!==1)fail(n,'expected one child');
      if(n.type==='Width'&&!('min'in n.props)&&!('max'in n.props))fail(n,'width rule needs a bound');
      if(n.type==='Grid'&&typeof n.props.columns==='number'&&!Number.isInteger(n.props.columns))fail(n,'columns must be integral');
      if(n.type==='For'&&!(n.props.key?.bind?.startsWith('item.')))fail(n,'key must be item path');
      if(n.type==='Slider'&&typeof n.props.min==='number'&&typeof n.props.max==='number'&&n.props.max<=n.props.min)fail(n,'invalid slider range');
      if(n.type==='Use'&&typeof n.props.definition!=='string')fail(n,'definition must be literal');
      for(const child of n.children){
        if(['Screen','Surface','Dashboard'].includes(child.type)&&n.type!=='Dashboard')fail(child,'invalid structural nesting');
        visit(child,depth+1,inItem||n.type==='For');
      }
    }
    visit(tree.root);
    if(tree.root.type!=='Dashboard')fail(tree.root,'root must be Dashboard');
    if(tree.root.children.filter(n=>n.type==='Surface').length!==1||tree.root.children.some(n=>!['Screen','Surface'].includes(n.type)))fail(tree.root,'Dashboard needs screens and one Surface');
    const screens=new Set(tree.root.children.filter(n=>n.type==='Screen').map(n=>n.id));
    let referenceCount=0;
    function references(n,stack=[]){
      if(++referenceCount>4096)fail(n,'reference expansion limit');
      if(n.type==='ScreenRef'&&typeof n.props.screen==='string'&&!screens.has(n.props.screen))fail(n,'unknown screen');
      if(n.type==='Use'){
        const name=n.props.definition;
        if(!Object.hasOwn(definitions,name))fail(n,'unknown UI definition '+name);
        if(stack.includes(name))fail(n,'UI reference cycle');
        references(definitions[name],stack.concat(name));
      }
      n.children.forEach(c=>references(c,stack));
    }
    // Definition validation is performed when a package is linked; compile may
    // produce references before the corresponding registry is supplied.
    if(arguments.length>1){
      if(!definitions||typeof definitions!=='object'||Array.isArray(definitions)||Object.keys(definitions).length>64)throw Error('definition registry limit/type');
      for(const definition of Object.values(definitions)){ids.clear();count=0;visit(definition);if(['Dashboard','Screen','Surface'].includes(definition.type))fail(definition,'definition must be a ViewGroup or View');}
      references(tree.root);
      for(const [name,definition]of Object.entries(definitions))references(definition,[name]);
    }
    return tree;
  }
  function lookup(path,scope,n){
    let v=scope;for(const part of path.split('.')){if(v===null||typeof v!=='object'||!Object.hasOwn(v,part))fail(n,'missing binding '+path);v=v[part];}return v;
  }
  function px(value,scale){return parseFloat(value)*(value.endsWith('dp')?scale:1);}
  function resolve(tree,state,{params={},definitions={},width=1024,scale=1}={}){
    validate(tree,definitions);
    if(!Number.isFinite(width)||width<0||!Number.isFinite(scale)||scale<=0)throw Error('invalid client dimensions');
    const screens=new Map(tree.root.children.filter(n=>n.type==='Screen').map(n=>[n.id,n]));let count=0;
    function walk(n,scope,path,stack=[]){
      if(++count>4096||stack.length>64)fail(n,'expanded UI limit');
      const id=path+'/'+n.id,p={};
      for(const [key,v]of Object.entries(n.props)){
        if(n.type==='For'&&key==='key'){p[key]=v;continue;}
        if(v&&typeof v==='object'&&v.bind){const value=lookup(v.bind,scope,n);if(v.not&&typeof value!=='boolean')fail(n,'negation requires boolean');p[key]=v.not?!value:value;}else p[key]=v;
        const exceptional=x=>x===undefined||x===null||typeof x==='number'&&!Number.isFinite(x);
        const label=x=>x===Infinity?'∞':x===-Infinity?'−∞':String(x);
        if(['Text','Status'].includes(n.type)&&key==='text'&&exceptional(p[key]))p[key]=label(p[key]);
        if(['Slider','Switch'].includes(n.type)&&key==='value'&&exceptional(p[key])){p.unavailable=label(p[key]);continue;}
        if(n.type==='Chart'&&key==='values'&&Array.isArray(p[key])){p.sampleLabels=p[key].map(label);p[key]=p[key].map(x=>exceptional(x)?null:x);}
        checkValue(schema[n.type][key],p[key],n);
      }
      if(n.type==='Switch'&&p.unavailable!==undefined){p.value=false;p.enabled=false;p.label+=': '+p.unavailable+' (unavailable)';}
      if(n.type==='Slider'&&p.unavailable!==undefined){p.value=p.min;p.enabled=false;p.label+=': '+p.unavailable+' (unavailable)';}
      if(n.type==='Slider'&&(p.max<=p.min||p.value<p.min||p.value>p.max))fail(n,'invalid slider range/value');
      if(n.type==='Grid'&&p.columns!==undefined&&!Number.isInteger(p.columns))fail(n,'columns must be integral');
      if(n.type==='ScreenRef'){
        if(!screens.has(p.screen))fail(n,'unknown screen '+p.screen);
        if(stack.includes('screen:'+p.screen))fail(n,'screen reference cycle');
        return {type:'Column',id,props:{},children:[walk(screens.get(p.screen),scope,id,[...stack,'screen:'+p.screen])]};
      }
      if(n.type==='Use'){
        if(stack.includes('use:'+p.definition))fail(n,'UI reference cycle');
        return {type:'Column',id,props:{},children:[walk(definitions[p.definition],{...scope,params:p.params},id,[...stack,'use:'+p.definition])]};
      }
      if(n.type==='For'){
        const seen=new Set();const children=p.items.map(item=>{
          const inner={...scope,item},key=lookup(p.key.bind,inner,n);
          if(!(typeof key==='string'&&key.length||typeof key==='number'&&Number.isFinite(key)))fail(n,'missing/invalid repeated key');
          const token=JSON.stringify([typeof key,key]);if(seen.has(token))fail(n,'duplicate repeated key');seen.add(token);
          return walk(n.children[0],inner,id+'/'+encodeURIComponent(token),stack);
        });return {type:'Column',id,props:{},children};
      }
      if(n.type==='If'&&!p.when||n.type==='Width'&&(('min'in p&&width<px(p.min,scale))||('max'in p&&width>=px(p.max,scale))))return {type:'Column',id,props:{visibility:'collapsed'},children:[]};
      const type=['Screen','Surface','If','Width'].includes(n.type)?'Column':n.type;
      return {type,id,props:p,children:n.children.map(c=>walk(c,scope,id,stack))};
    }
    return walk(tree.root.children.find(n=>n.type==='Surface'),{state,params},tree.root.id);
  }
  function compileDefinition(source){return compile('<Dashboard id="DefinitionRoot"><Surface id="DefinitionSurface">'+source+'</Surface></Dashboard>').root.children[0].children[0];}
  function validatePackage(pkg){
    if(!pkg||pkg.version!==1||typeof pkg.id!=='string'||!pkg.id||typeof pkg.revision!=='string'||!pkg.revision||typeof pkg.viewModel!=='string'||pkg.viewModel.length>131072)throw Error('invalid dashboard package');
    if(pkg.grants){if(typeof pkg.grants!=='object'||Array.isArray(pkg.grants))throw Error('invalid grants');for(const [kind,ids]of Object.entries(pkg.grants)){if(!['reads','writes','runs'].includes(kind)||!Array.isArray(ids)||ids.length>64||new Set(ids).size!==ids.length||ids.some(id=>typeof id!=='string'||!/^[A-Za-z0-9_.-]{1,128}$/.test(id)))throw Error('invalid resource grants');}}
    validate(pkg.ui,pkg.definitions||{});return pkg;
  }
  globalThis.TaliaUI=Object.freeze({compile,compileDefinition,validate,validatePackage,resolve,px,schema});
})();
