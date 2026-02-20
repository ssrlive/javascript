// feature probe for 'Atomics.pause'
try {
  if (typeof Atomics !== 'object') throw new Error('Atomics missing');
  if (typeof Atomics.pause !== 'function') throw new Error('Atomics.pause missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
