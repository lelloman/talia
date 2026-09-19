// Prototype support layer, installed by the host before dashboard code.
// JSON-only messages cross the boundary; no host objects enter guest JS.
(() => {
  const pending = new Map(), listeners = new Map();
  let next = 0;
  function json(value) {
    const seen = new Set();
    function check(v) {
      if (v === null || typeof v === 'string' || typeof v === 'boolean') return;
      if (typeof v === 'number' && Number.isFinite(v)) return;
      if (typeof v !== 'object' || seen.has(v)) throw Error('unsupported wire value');
      if (!Array.isArray(v) && Object.getPrototypeOf(v) !== Object.prototype)
        throw Error('unsupported wire object');
      seen.add(v);
      for (const x of Object.values(v)) check(x);
      seen.delete(v);
    }
    check(value);
    return JSON.stringify(value);
  }
  function call(op, value = null) {
    const id = ++next;
    const message = json({ id, op, value });
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      __send(message);
    });
  }
  globalThis.__receive = raw => {
    const msg = JSON.parse(raw);
    if (msg.event) { listeners.get(msg.event)?.(msg.value); return; }
    const p = pending.get(msg.id);
    if (!p) return; // Duplicate/cancelled response.
    pending.delete(msg.id);
    msg.error ? p.reject(Error(msg.error)) : p.resolve(msg.value);
  };
  globalThis.engine = Object.freeze({
    read: key => call('read', key),
    write: value => call('write', value),
    call,
    async subscribe(key, listener) {
      const id = await call('subscribe', key);
      listeners.set(id, listener);
      return async () => { listeners.delete(id); await call('unsubscribe', id); };
    },
    cancelPending() {
      for (const p of pending.values()) p.reject(Error('cancelled'));
      pending.clear();
    },
  });
  globalThis.clock = Object.freeze({ now: () => 1000 });
  globalThis.vm = { state: { value: 1 }, action: () => 2 };
  globalThis.report = { done: false, checks: [] };
  globalThis.assert = (condition, name) => {
    if (!condition) throw Error(name);
    report.checks.push(name);
  };
})();

// Candidate API for trusted fixtures, not a production permission boundary.
// No operation queue: awaiting yields even to operations on the same instance.
globalThis.ExecutionScheduler = class {
  #cells = new Map();
  #tasks = new Set();
  #snapshots = new WeakMap();
  #disposed = false;
  #copy(value) {
    const seen = new Set();
    const visit = v => {
      if (v === null || typeof v === 'string' || typeof v === 'boolean') return;
      if (typeof v === 'number' && Number.isFinite(v)) return;
      if (typeof v !== 'object' || seen.has(v) ||
          (!Array.isArray(v) && Object.getPrototypeOf(v) !== Object.prototype))
        throw Error('state must be JSON');
      seen.add(v);
      // Do not execute user accessors during a synchronous state update.
      for (const d of Object.values(Object.getOwnPropertyDescriptors(v))) {
        if (d.get || d.set) throw Error('state must be JSON');
        visit(d.value);
      }
      seen.delete(v);
    };
    visit(value);
    return JSON.parse(JSON.stringify(value));
  }
  define(key, {initial = null, readMode, get}) {
    if (this.#disposed) throw Error('disposed');
    if (!['shared', 'independent'].includes(readMode) || typeof get !== 'function')
      throw Error('explicit read mode and getter required');
    const value = this.#copy(initial);
    const cell = this.#cells.get(key) || {revision:0};
    Object.assign(cell, {value, get, readMode, revision:cell.revision+1, shared:null});
    this.#cells.set(key, cell);
  }
  #cell(key) {
    if (this.#disposed) throw Error('disposed');
    const cell = this.#cells.get(key);
    if (!cell) throw Error('unknown instance');
    return cell;
  }
  invalidate(key) { const cell=this.#cell(key); cell.revision++; cell.shared=null; }
  #reaches(from, target, seen = new Set()) {
    if (from === target) return true;
    if (seen.has(from)) return false;
    seen.add(from);
    return [...from.edges.keys()].some(next => this.#reaches(next, target, seen));
  }
  #check(task) {
    if (task.cancelled) throw Error('cancelled');
    if (task.finished) throw Error('operation finished');
    if (task.cell.revision !== task.revision) throw Error('stale operation');
  }
  #cancel(task) {
    task.cancelled = true;
    if (task.cell.shared === task) task.cell.shared=null;
    for (const lease of [...task.children]) lease.cancel();
  }
  #lease(task, parent) {
    if (parent && this.#reaches(task, parent)) throw Error('dependency cycle');
    if (parent) parent.edges.set(task, (parent.edges.get(task)||0)+1);
    let resolve, reject, done=false;
    const promise = new Promise((yes,no) => {resolve=yes;reject=no;});
    const finish = (success,value) => {
      if (done) return;
      done=true;
      task.readers.delete(lease);
      if (parent) {
        parent.children.delete(lease);
        const n=parent.edges.get(task)-1;
        n ? parent.edges.set(task,n) : parent.edges.delete(task);
      }
      success ? resolve(value) : reject(value);
    };
    const lease = {promise, cancel:() => {
      finish(false,Error('cancelled'));
      if (!task.finished && !task.readers.size) this.#cancel(task);
    }, finish};
    task.readers.add(lease);
    if (parent) parent.children.add(lease);
    return {promise, cancel:lease.cancel};
  }
  #launch(key, action, parent, shared) {
    const cell=this.#cell(key);
    if (parent) this.#check(parent);
    if (parent?.path.includes(key)) throw Error('dependency cycle');
    if (shared && cell.shared && !cell.shared.cancelled &&
        cell.shared.revision===cell.revision) return this.#lease(cell.shared,parent);
    const task = {key, cell, revision:cell.revision, cancelled:false, finished:false,
      edges:new Map(), children:new Set(), readers:new Set(), path:[...(parent?.path||[]),key]};
    const handle=this.#lease(task,parent);
    this.#tasks.add(task);
    if (shared) cell.shared=task;
    const check=() => this.#check(task);
    const context=Object.freeze({
      check,
      snapshot:() => {
        check();
        const snapshot=Object.freeze({value:this.#copy(cell.value)});
        this.#snapshots.set(snapshot,{cell,revision:cell.revision});
        return snapshot;
      },
      commit:(snapshot,value) => {
        check();
        const stamp=this.#snapshots.get(snapshot);
        if (!stamp || stamp.cell!==cell || stamp.revision!==cell.revision)
          throw Error('stale snapshot');
        const copy=this.#copy(value);
        // Recheck after validation; trusted plain JSON is the supported input.
        check();
        if (stamp.revision!==cell.revision) throw Error('stale snapshot');
        cell.value=copy;
        task.revision=++cell.revision;
        cell.shared=null;
      },
      read:other => this.#launch(other,this.#cell(other).get,task,
        this.#cell(other).readMode==='shared').promise,
      effect:action => { check(); return action(); },
    });
    Promise.resolve().then(async () => {
      check();
      const result=await action(context);
      check();
      return result;
    }).then(value => settle(true,value),error => settle(false,error));
    const settle=(success,value) => {
      // Promise adoption adds jobs between the last await and publication.
      if (success) {
        try { check(); } catch (error) { success=false; value=error; }
      }
      task.finished=true;
      if (cell.shared===task) cell.shared=null;
      this.#tasks.delete(task);
      // Detached dependencies are owned by this invocation, not by its instance.
      for (const child of [...task.children]) child.cancel();
      for (const lease of [...task.readers]) lease.finish(success,value);
    };
    return handle;
  }
  start(key, action) { return this.#launch(key,action,null,false); }
  read(key) { const c=this.#cell(key); return this.#launch(key,c.get,null,c.readMode==='shared'); }
  dispose() {
    this.#disposed=true;
    for (const task of this.#tasks) {
      this.#cancel(task);
      for (const lease of [...task.readers]) lease.finish(false,Error('cancelled'));
    }
    this.#cells.clear();
  }
  stats() {
    return {tasks:this.#tasks.size,
      edges:[...this.#tasks].reduce((n,t)=>n+t.edges.size,0),
      readers:[...this.#tasks].reduce((n,t)=>n+t.readers.size,0)};
  }
};
