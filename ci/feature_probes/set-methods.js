// feature probe for 'set-methods'
try {
  if (typeof Set !== 'function') throw new Error('Set missing');
  const a = new Set([1, 2, 3]);
  const b = new Set([2, 3, 4]);
  const u = a.union(b);
  if (!(u instanceof Set) || u.size !== 4) throw new Error('union failed');
  const i = a.intersection(b);
  if (i.size !== 2) throw new Error('intersection failed');
  const d = a.difference(b);
  if (d.size !== 1) throw new Error('difference failed');
  const sd = a.symmetricDifference(b);
  if (sd.size !== 2) throw new Error('symmetricDifference failed');
  if (!a.isSubsetOf(new Set([1,2,3,4]))) throw new Error('isSubsetOf failed');
  if (!a.isSupersetOf(new Set([1,2]))) throw new Error('isSupersetOf failed');
  if (!a.isDisjointFrom(new Set([4,5]))) throw new Error('isDisjointFrom failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
