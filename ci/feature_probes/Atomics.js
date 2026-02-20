// feature probe for 'Atomics'
try {
  if (typeof Atomics !== 'object') throw new Error('Atomics missing');
  if (typeof Atomics.load !== 'function') throw new Error('Atomics.load missing');
  if (typeof Atomics.store !== 'function') throw new Error('Atomics.store missing');
  if (typeof Atomics.compareExchange !== 'function') throw new Error('Atomics.compareExchange missing');
  if (typeof Atomics.isLockFree !== 'function') throw new Error('Atomics.isLockFree missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
