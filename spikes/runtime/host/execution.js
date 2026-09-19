// Trusted host code. Never load this authority into an authored-script context.
// IDs are host routing handles, never authority supplied by guest requests.
globalThis.ExecutionAuthority = class {
  #cells = new Map(); #tasks = new Map(); #leases = new Map(); #next = 0;
  #copy(value) {
    const seen = new Set();
    const visit = v => {
      if (v===null || typeof v==='string' || typeof v==='boolean') return;
      if (typeof v==='number' && Number.isFinite(v)) return;
      if (!v || typeof v!=='object' || seen.has(v) || (!Array.isArray(v) && Object.getPrototypeOf(v)!==Object.prototype)) throw Error('JSON state required');
      seen.add(v);
      for(const d of Object.values(Object.getOwnPropertyDescriptors(v))) {
        if(d.get || d.set) throw Error('JSON state required'); visit(d.value);
      }
      seen.delete(v);
    };
    visit(value); return JSON.parse(JSON.stringify(value));
  }
  define(key, mode, initial=null) {
    if(!['shared','independent'].includes(mode)) throw Error('explicit read mode required');
    const value=this.#copy(initial), old=this.#cells.get(key);
    this.#cells.set(key,{mode,value,revision:(old?.revision||0)+1,shared:null});
  }
  #cell(key) {const c=this.#cells.get(key);if(!c)throw Error('unknown instance');return c;}
  check(id) {
    const t=this.#tasks.get(id);
    if(!t || t.finished) throw Error('operation finished');
    if(t.cancelled) throw Error('cancelled');
    if(this.#cell(t.key).revision!==t.revision) throw Error('stale evaluation');
    return t;
  }
  #reaches(from,to,seen=new Set()) {
    if(from===to)return true;if(seen.has(from))return false;seen.add(from);
    return [...this.#leases.values()].some(l=>!l.done && l.parent===from && this.#reaches(l.task,to,seen));
  }
  begin(key, kind='read', parent=null) {
    if(!['read','write'].includes(kind))throw Error('operation kind');
    const cell=this.#cell(key), p=parent===null?null:this.check(parent);
    if(p?.path.includes(key))throw Error('dependency cycle');
    if(this.#tasks.size>=128 || this.#leases.size>=256)throw Error('execution capacity');
    let task=kind==='read' && cell.mode==='shared' ? this.#tasks.get(cell.shared):null;
    if(task && (task.finished || task.cancelled || task.revision!==cell.revision))task=null;
    if(task && p && this.#reaches(task.id,parent))throw Error('dependency cycle');
    const start=!task;
    if(!task){task={id:++this.#next,key,revision:cell.revision,cancelled:false,finished:false,path:[...(p?.path||[]),key]};this.#tasks.set(task.id,task);if(kind==='read' && cell.mode==='shared')cell.shared=task.id;}
    const lease=++this.#next;this.#leases.set(lease,{task:task.id,parent,done:false});
    return {task:task.id,lease,start};
  }
  snapshot(id) {const t=this.check(id);return this.#copy(this.#cell(t.key).value);}
  commit(id,value) {
    const t=this.check(id), copy=this.#copy(value);this.check(id);
    const cell=this.#cell(t.key);cell.value=copy;t.revision=++cell.revision;cell.shared=null;
    return this.#copy(copy);
  }
  invalidate(key) {const c=this.#cell(key);c.revision++;c.shared=null;}
  #endLease(id,error,value=null) {
    const l=this.#leases.get(id);if(!l || l.done)return;
    l.done=true;l.error=error;l.value=value;
  }
  cancel(lease) {
    const l=this.#leases.get(lease);if(!l || l.done)return;
    this.#endLease(lease,'cancelled');
    if(![...this.#leases.values()].some(other=>other.task===l.task && !other.done)) {
      const t=this.#tasks.get(l.task);if(!t)return;t.cancelled=true;
      const c=this.#cell(t.key);if(c.shared===t.id)c.shared=null;
      for(const [id,child] of this.#leases)if(child.parent===t.id && !child.done)this.cancel(id);
    }
  }
  settle(id,value=null,error=null) {
    const t=this.#tasks.get(id);if(!t || t.finished)return;
    if(error===null){try{this.check(id);value=this.#copy(value);}catch(e){error=e.message;value=null;}}
    t.finished=true;
    const c=this.#cell(t.key);if(c.shared===id)c.shared=null;
    for(const [lease,l] of this.#leases)if(l.parent===id && !l.done)this.cancel(lease);
    for(const [lease,l] of this.#leases)if(l.task===id)this.#endLease(lease,error,value);
    this.#tasks.delete(id);
  }
  outcome(lease) {
    const l=this.#leases.get(lease);if(!l)throw Error('unknown reader');
    return l.done?this.#copy({done:true,error:l.error,value:l.value}):{done:false};
  }
  release(lease) {const l=this.#leases.get(lease);if(l && !l.done)throw Error('reader pending');this.#leases.delete(lease);}
  stats(){return {tasks:this.#tasks.size,readers:[...this.#leases.values()].filter(l=>!l.done).length};}
};
