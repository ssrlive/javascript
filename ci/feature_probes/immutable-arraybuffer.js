// feature probe for 'immutable-arraybuffer'
// This proposal-level feature is considered supported only if an explicit
// immutable ArrayBuffer marker exists on the prototype.
try {
  const proto = (typeof ArrayBuffer === 'function') ? ArrayBuffer.prototype : null;
  if (!proto || typeof proto !== 'object') throw new Error('ArrayBuffer missing');
  if (typeof proto.immutable === 'undefined') throw new Error('immutable marker missing');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
