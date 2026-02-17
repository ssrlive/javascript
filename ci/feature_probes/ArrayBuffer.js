// feature probe for 'ArrayBuffer'
try {
  if (typeof ArrayBuffer !== 'function') throw new Error('ArrayBuffer missing');
  const buf = new ArrayBuffer(8);
  if (!buf || typeof buf !== 'object') throw new Error('ArrayBuffer construction failed');
  if (buf.byteLength !== 8) throw new Error('ArrayBuffer byteLength mismatch');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
