// feature probe for 'WeakRef'
try {
  if (typeof WeakRef !== 'function') throw new Error('WeakRef missing');
  const wr = new WeakRef({a: 1});
  if (!wr) throw new Error('WeakRef failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
