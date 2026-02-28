// feature probe for 'upsert'
try {
  if (typeof Map !== 'function') throw new Error('Map missing');
  const m = new Map();
  const v1 = m.getOrInsert('a', 1);
  if (v1 !== 1) throw new Error('getOrInsert failed');
  const v2 = m.getOrInsert('a', 2);
  if (v2 !== 1) throw new Error('getOrInsert should return existing');
  const v3 = m.getOrInsertComputed('b', () => 42);
  if (v3 !== 42) throw new Error('getOrInsertComputed failed');
  const v4 = m.getOrInsertComputed('b', () => 99);
  if (v4 !== 42) throw new Error('getOrInsertComputed should return existing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
