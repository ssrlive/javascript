// feature probe for 'align-detached-buffer-semantics-with-web-reality'
// Conservative probe: require explicit detached accessor support.
try {
  const proto = (typeof ArrayBuffer === 'function') ? ArrayBuffer.prototype : null;
  if (!proto || typeof proto !== 'object') throw new Error('ArrayBuffer missing');
  const desc = Object.getOwnPropertyDescriptor(proto, 'detached');
  if (!desc || typeof desc.get !== 'function') throw new Error('detached accessor missing');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
