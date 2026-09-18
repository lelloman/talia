// Spike defaults, not signed-off product limits. Shared by trusted Worker and parent.
export const LIMITS = Object.freeze({
  wireBytes:32768, commandBytes:65536, depth:16, nodes:2048, inFlight:32, commands:64,
  subscriptions:16, timers:16, stalled:16, workers:8,
});
const encoder = new TextEncoder();
export function boundedText(raw, limit = LIMITS.wireBytes) {
  if (typeof raw !== 'string' || raw.length > limit ||
      encoder.encode(raw).byteLength > limit) throw Error('wire size/type');
  return raw;
}
export function validateRequest(raw) {
  const request = JSON.parse(boundedText(raw));
  let nodes = 0;
  function visit(value, depth) {
    if (++nodes > LIMITS.nodes || depth > LIMITS.depth) throw Error('JSON complexity');
    if (value === null || typeof value === 'string' || typeof value === 'boolean') return;
    if (typeof value === 'number' && Number.isFinite(value)) return;
    if (!value || typeof value !== 'object') throw Error('JSON value');
    for (const child of Object.values(value)) visit(child, depth + 1);
  }
  visit(request, 0);
  if (!request || Array.isArray(request) || typeof request !== 'object' ||
      Object.keys(request).sort().join(',') !== 'id,op,value' ||
      !Number.isSafeInteger(request.id) || request.id < 1) throw Error('request envelope');
  const {op, value} = request;
  switch (op) {
    case 'echo': break;
    case 'read': case 'subscribe':
      if (value !== 'value') throw Error('unknown variable'); break;
    case 'write': case 'publish':
      if (typeof value !== 'number' || !Number.isFinite(value)) throw Error('numeric value required'); break;
    case 'unsubscribe':
      if (typeof value !== 'string' || !/^s[1-9][0-9]*$/.test(value)) throw Error('subscription identifier'); break;
    case 'delay':
      if (value !== null && value !== 'long') throw Error('delay argument'); break;
    case 'fail': case 'stall': case 'late':
      if (value !== null) throw Error('null argument required'); break;
    default: throw Error('unknown capability');
  }
  return request;
}
