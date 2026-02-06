// feature probe for 'Set'
try {
  if (typeof Set !== 'function') throw new Error('Set missing');
  const s = new Set();
  s.add(1);
  if (!s.has(1)) throw new Error('Set failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
