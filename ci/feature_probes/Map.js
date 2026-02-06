// feature probe for 'Map'
try {
  if (typeof Map !== 'function') throw new Error('Map missing');
  const m = new Map();
  m.set('a', 1);
  if (m.get('a') !== 1) throw new Error('Map failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
