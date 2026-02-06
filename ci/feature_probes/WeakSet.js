// feature probe for 'WeakSet'
try {
  if (typeof WeakSet !== 'function') throw new Error('WeakSet missing');
  const ws = new WeakSet();
  const k = {};
  ws.add(k);
  if (!ws.has(k)) throw new Error('WeakSet failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
