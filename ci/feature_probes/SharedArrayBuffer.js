// feature probe for 'SharedArrayBuffer'
try {
  if (typeof SharedArrayBuffer !== 'function') throw new Error('SharedArrayBuffer missing');
  const buf = new SharedArrayBuffer(8);
  if (!buf) throw new Error('SharedArrayBuffer failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
