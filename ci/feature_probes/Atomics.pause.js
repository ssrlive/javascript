// Feature probe: Atomics.pause
try {
  if (typeof Atomics !== 'object') throw new Error('Atomics missing');
  if (typeof Atomics.pause !== 'function') throw new Error('pause missing');
  Atomics.pause();
  console.log('OK');
} catch(e) {
  console.log('NO');
}
