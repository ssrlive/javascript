// feature probe for 'TypedArray'
try {
  if (typeof Int8Array !== 'function') throw new Error('Int8Array missing');
  const a = new Int8Array(1);
  a[0] = 1;
  if (a[0] !== 1) throw new Error('TypedArray failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
