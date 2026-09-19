// Prototype support layer, installed by the host before dashboard code.
// JSON-only messages cross the boundary; no host objects enter guest JS.
(() => {
  const pending = new Map(), listeners = new Map(), queues = new Map();
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
  globalThis.serial = (id, action) => {
    const result = (queues.get(id) || Promise.resolve()).then(action);
    queues.set(id, result.catch(() => {}));
    return result;
  };
  globalThis.clock = Object.freeze({ now: () => 1000 });
  globalThis.vm = { state: { value: 1 }, action: () => 2 };
  globalThis.report = { done: false, checks: [] };
  globalThis.assert = (condition, name) => {
    if (!condition) throw Error(name);
    report.checks.push(name);
  };
})();

// Candidate execution semantics for trusted fixtures, not a production sandbox API.
globalThis.ExecutionScheduler = class {
  constructor() { this.tasks = new Set(); this.tails = new Map(); }
  start(key, action, parent = null) {
    const previous = this.tails.get(key);
    const task = {key, cancelled:false, edges:new Set(), children:new Set(), promise:null};
    if (previous) task.edges.add(previous);
    this.tasks.add(task);
    const reaches = (from, target, seen = new Set()) => {
      if (from === target) return true;
      if (seen.has(from)) return false;
      seen.add(from);
      return [...from.edges].some(next => reaches(next, target, seen));
    };
    if (parent && reaches(task, parent)) {
      this.tasks.delete(task);
      return {promise:Promise.reject(Error('dependency cycle')), cancel() {}};
    }
    if (parent) { parent.edges.add(task); parent.children.add(task); }
    const check = () => { if (task.cancelled) throw Error('cancelled'); };
    const context = Object.freeze({
      check,
      read: (other, getter) => {
        check();
        return this.start(other, getter, task).promise;
      },
      commit: update => { check(); return update(); },
      effect: action => { check(); return action(); },
    });
    const predecessor = previous ? previous.promise.catch(() => {}) : Promise.resolve();
    task.promise = predecessor.then(async () => {
      check();
      const value = await action(context);
      check();
      return value;
    }).finally(() => {
      this.tasks.delete(task);
      if (parent) parent.children.delete(task);
      for (const current of this.tasks) current.edges.delete(task);
      if (this.tails.get(key) === task) this.tails.delete(key);
    });
    this.tails.set(key, task);
    const cancel = current => {
      current.cancelled = true;
      for (const child of current.children) cancel(child);
    };
    return {promise:task.promise, cancel:() => cancel(task)};
  }
};
