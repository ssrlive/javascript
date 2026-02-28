// feature probe for 'change-array-by-copy'
try {
  const arr = [3, 1, 2];
  const r = arr.toReversed();
  if (r[0] !== 2 || r[1] !== 1 || r[2] !== 3) throw new Error('toReversed failed');
  if (arr[0] !== 3) throw new Error('toReversed mutated original');
  const s = arr.toSorted();
  if (s[0] !== 1 || s[1] !== 2 || s[2] !== 3) throw new Error('toSorted failed');
  const sp = arr.toSpliced(1, 1, 9);
  if (sp[0] !== 3 || sp[1] !== 9 || sp[2] !== 2 || sp.length !== 3) throw new Error('toSpliced failed');
  const w = arr.with(1, 42);
  if (w[0] !== 3 || w[1] !== 42 || w[2] !== 2) throw new Error('with failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
