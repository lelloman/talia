/* Versioned portable value codec. Loaded unchanged in browser and native QuickJS. */
(() => {
  const MAX=131072, DEPTH=48, NODES=16000;
  function encode(value) {
    let count=0; const seen=new Set();
    function node(v,d) {
      if(++count>NODES||d>DEPTH)throw Error('value complexity limit');
      if(v===undefined)return ['undefined'];
      if(v===null)return ['null'];
      if(typeof v==='boolean')return ['boolean',v];
      if(typeof v==='string')return ['string',v];
      if(typeof v==='number')return ['number',Number.isNaN(v)?'NaN':v===Infinity?'Infinity':v===-Infinity?'-Infinity':Object.is(v,-0)?'-0':v];
      if(typeof v!=='object'||seen.has(v))throw Error('unsupported or cyclic value');
      if(!Array.isArray(v)&&Object.getPrototypeOf(v)!==Object.prototype&&Object.getPrototypeOf(v)!==null)throw Error('plain object required');
      const descriptors=Object.getOwnPropertyDescriptors(v);
      if(Reflect.ownKeys(v).some(k=>typeof k==='symbol')||Object.values(descriptors).some(x=>x.get||x.set))throw Error('accessors/symbols forbidden');
      seen.add(v);let result;
      if(Array.isArray(v)) {
        if(v.length>NODES||Object.keys(v).some(k=>!/^(0|[1-9][0-9]*)$/.test(k)||Number(k)>=v.length))throw Error('array properties');
        result=['array',Array.from({length:v.length},(_,i)=>node(v[i],d+1))];
      } else result=['object',Object.keys(descriptors).sort().map(k=>[k,node(descriptors[k].value,d+1)])];
      seen.delete(v);return result;
    }
    const out={version:1,value:node(value,0)};
    if(JSON.stringify(out).length>MAX/3) {
      // UTF-8 upper bound without depending on TextEncoder in embedded runtimes.
      const bytes=unescape(encodeURIComponent(JSON.stringify(out))).length;
      if(bytes>MAX)throw Error('value size limit');
    }
    return out;
  }
  function decode(wire) {
    let count=0;
    if(!wire||Object.keys(wire).sort().join(',')!=='value,version'||wire.version!==1)throw Error('value version/envelope');
    function node(n,d) {
      if(++count>NODES||d>DEPTH||!Array.isArray(n))throw Error('value complexity/shape');
      const [t,v]=n;
      if(n.length===1&&t==='undefined')return undefined;
      if(n.length===1&&t==='null')return null;
      if(n.length!==2)throw Error('value arity');
      if(t==='boolean'&&typeof v==='boolean'||t==='string'&&typeof v==='string')return v;
      if(t==='number') {
        if(typeof v==='number'&&Number.isFinite(v)&&!Object.is(v,-0))return v;
        if(v==='NaN')return NaN;if(v==='Infinity')return Infinity;if(v==='-Infinity')return -Infinity;if(v==='-0')return -0;
      }
      if(t==='array'&&Array.isArray(v))return v.map(x=>node(x,d+1));
      if(t==='object'&&Array.isArray(v)) {
        const result={},keys=new Set();
        for(const pair of v) {
          if(!Array.isArray(pair)||pair.length!==2||typeof pair[0]!=='string'||keys.has(pair[0]))throw Error('object entry');
          keys.add(pair[0]);Object.defineProperty(result,pair[0],{value:node(pair[1],d+1),writable:true,enumerable:true,configurable:true});
        }
        return result;
      }
      throw Error('unknown/invalid value tag');
    }
    const result=node(wire.value,0);encode(result);return result;
  }
  const stringify=v=>JSON.stringify(encode(v));
  Object.defineProperty(globalThis,'TaliaValue',{value:Object.freeze({encode,decode,stringify,parse:s=>{if(s.length>MAX)throw Error('value size limit');return decode(JSON.parse(s));},copy:v=>decode(encode(v)),equal:(a,b)=>stringify(a)===stringify(b)})});
})();
