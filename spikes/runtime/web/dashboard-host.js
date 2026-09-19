import '../host/execution.js';
import {LIMITS, boundedText, validateRequest} from './bridge-policy.js';
// Parent-owned resources survive Worker failures and are retired by generation.
export class DashboardHost {
  #grants = new WeakMap();
  #execution = new WeakMap();
  bindExecution(guest, authority, task) { this.#execution.set(guest,{authority,task}); }
  #views = new WeakMap();
  view(guest) { return structuredClone(this.#views.get(guest)); }
  value = 0;
  next = 0;
  generation = 0;
  guests = new Map();
  subscriptions = new Map();
  stalled = [];
  timers = new Map();

  async create(bridge, grants = ['echo','read','write','subscribe','unsubscribe','publish','delay','fail','stall','late'], view = {type:'Text',text:'saved'}) {
    if (this.guests.size >= LIMITS.workers) throw Error('Worker limit');
    const generation = ++this.generation;
    const worker = new Worker('/web/dist/dashboard-worker.js', {type:'module'});
    const pending = new Map();
    let next = 0;
    const guest = {generation, worker, pending, active:true, memory_bytes:0, reason:null, lastRequest:0};
    this.#grants.set(guest, new Set(grants));
    this.#views.set(guest, structuredClone(view));
    guest.command = (op, value, timeout = 3000) => new Promise((resolve, reject) => {
      if (!guest.active) { reject(Error('retired Worker')); return; }
      const id = ++next;
      try {
        boundedText(JSON.stringify({id, op, value}), LIMITS.commandBytes);
        if (pending.size >= LIMITS.commands) throw Error('command queue limit');
      } catch (error) { this.retire(guest, 'bridge policy: ' + error.message); reject(error); return; }
      const timer = setTimeout(() => this.retire(guest, 'watchdog'), timeout);
      pending.set(id, {resolve, reject, timer});
      worker.postMessage({id, op, value});
    });
    guest.eval = source => guest.command('eval', source);
    worker.onmessage = ({data}) => {
      if (!guest.active) return;
      if (data.hanging) { guest.hanging = true; return; }
      if (data.memory_bytes) guest.memory_bytes = data.memory_bytes;
      if (data.fatal) { this.retire(guest, 'guest failure: ' + data.fatal); return; }
      if (Object.hasOwn(data, 'request')) {
        this.request(guest, data.request);
        if (guest.active) worker.postMessage({op:'ack'});
        return;
      }
      const operation = pending.get(data.id);
      if (!operation) return;
      clearTimeout(operation.timer); pending.delete(data.id); operation.resolve(data.value);
    };
    worker.onerror = event => { event.preventDefault(); this.retire(guest, 'Worker error'); };
    worker.onmessageerror = () => this.retire(guest, 'Worker message error');
    this.guests.set(generation, guest);
    await guest.command('init', bridge);
    return guest;
  }
  retire(guest, reason = 'reload') {
    if (!guest.active) return;
    guest.active = false; guest.reason = reason;
    guest.worker.terminate();
    this.guests.delete(guest.generation);
    for (const [id, generation] of this.subscriptions) {
      if (generation === guest.generation) this.subscriptions.delete(id);
    }
    this.stalled = this.stalled.filter(r => r.generation !== guest.generation);
    for (const [timer, generation] of this.timers) {
      if (generation === guest.generation) { clearTimeout(timer); this.timers.delete(timer); }
    }
    for (const p of guest.pending.values()) { clearTimeout(p.timer); p.reject(Error(reason)); }
    guest.pending.clear();
  }
  deliver(guest, message, generation = guest.generation) {
    if (!guest.active || generation !== guest.generation) return false;
    guest.command('deliver', message).catch(() => {}); // Retirement owns pending rejection.
    return true;
  }
  request(guest, raw) {
    if (!guest.active) return;
    let request;
    try {
      request = validateRequest(raw);
      if (!this.#grants.get(guest)?.has(request.op)) throw Error('operation not granted');
      const execution=this.#execution.get(guest);
      if(execution) execution.authority.check(execution.task);
      if (request.id <= guest.lastRequest) throw Error('request ID replay/order');
      guest.lastRequest = request.id;
      const owned = collection => [...collection.values()].filter(g => g === guest.generation).length;
      if (request.op === 'subscribe' && owned(this.subscriptions) >= LIMITS.subscriptions)
        throw Error('subscription limit');
      if (request.op === 'delay' && owned(this.timers) >= LIMITS.timers) throw Error('timer limit');
      if (request.op === 'stall' && this.stalled.filter(r => r.generation === guest.generation).length >= LIMITS.stalled)
        throw Error('stalled call limit');
      if (request.op === 'unsubscribe' && this.subscriptions.get(request.value) !== guest.generation)
        throw Error('subscription ownership');
      // Attribute publisher overload to the producer, before touching any recipient.
      if (request.op === 'publish') {
        const counts = new Map();
        for (const generation of this.subscriptions.values()) counts.set(generation, (counts.get(generation) || 0) + 1);
        for (const [generation, count] of counts) {
          const owner = this.guests.get(generation);
          if (owner && owner.pending.size + count + (owner === guest ? 1 : 0) > LIMITS.commands)
            throw Error('publish fanout limit');
        }
      }
    } catch (error) { this.retire(guest, 'bridge policy: ' + error.message); return; }
    const {id, op, value} = request;
    let result = null;
    switch (op) {
      case 'state.read': case 'state.commit': case 'state.result': {
        const execution=this.#execution.get(guest);
        if(!execution){this.retire(guest,'execution binding required');return;}
        try {
          const method=op==='state.read'?'snapshot':op==='state.commit'?'commit':'settle';
          result=execution.authority[method](execution.task,value)??null;
        } catch(error){this.retire(guest,error.message);return;}
        break;
      }
      case 'echo': result = value; break;
      case 'read': result = this.value; break;
      case 'write': result = this.value = value; break;
      case 'fail': this.deliver(guest, {id, error:'host failure'}); return;
      case 'subscribe': result = 's' + (++this.next); this.subscriptions.set(result, guest.generation); break;
      case 'unsubscribe':
        if (this.subscriptions.get(value) === guest.generation) this.subscriptions.delete(value);
        break;
      case 'publish':
        for (const [event, generation] of this.subscriptions) {
          const owner = this.guests.get(generation);
          if (owner) this.deliver(owner, {event, value}, generation);
        }
        break;
      case 'stall': this.stalled.push({generation:guest.generation, id}); return;
      case 'late': {
        const replies = this.stalled.filter(r => r.generation === guest.generation);
        this.stalled = this.stalled.filter(r => r.generation !== guest.generation);
        for (const reply of replies) this.deliver(guest, {id:reply.id, value:999}, reply.generation);
        break;
      }
      case 'delay': {
        const timer = setTimeout(() => {
          this.timers.delete(timer); this.deliver(guest, {id, value:null});
        }, value === 'long' ? 10000 : 5);
        this.timers.set(timer, guest.generation); return;
      }
      default: this.retire(guest, 'unknown capability'); return;
    }
    this.deliver(guest, {id, value:result});
    if (op === 'late') this.deliver(guest, {id, value:result});
  }
  close() { for (const guest of [...this.guests.values()]) this.retire(guest, 'harness closed'); }
}
