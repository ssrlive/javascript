// feature probe for 'Atomics.waitAsync'
try {
  if (typeof Atomics !== 'object') throw new Error('Atomics missing');
  if (typeof Atomics.waitAsync !== 'function') throw new Error('Atomics.waitAsync missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
