// feature probe for 'WeakMap'
try {
  if (typeof WeakMap !== 'function') throw new Error('WeakMap missing');
  const wm = new WeakMap();
  const k = {};
  wm.set(k, 1);
  if (wm.get(k) !== 1) throw new Error('WeakMap failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
