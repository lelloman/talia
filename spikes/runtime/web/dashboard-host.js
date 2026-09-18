// Parent-owned resources survive Worker failures and are retired by generation.
export class DashboardHost {
  value = 0;
  next = 0;
  generation = 0;
  guests = new Map();
  subscriptions = new Map();
  stalled = [];
  timers = new Map();

  async create(bridge) {
    const generation = ++this.generation;
    const worker = new Worker('/web/dist/dashboard-worker.js', {type:'module'});
    const pending = new Map();
    let next = 0;
    const guest = {generation, worker, pending, active:true, memory_bytes:0, reason:null};
    guest.command = (op, value, timeout = 3000) => new Promise((resolve, reject) => {
      if (!guest.active) { reject(Error('retired Worker')); return; }
      const id = ++next;
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
      if (data.request) { this.request(guest, data.request); return; }
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
  request(guest, {id, op, value}) {
    if (!guest.active) return;
    let result = null;
    switch (op) {
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
