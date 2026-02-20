// feature probe for 'SharedArrayBuffer'
try {
  if (typeof SharedArrayBuffer === 'undefined') throw new Error('SharedArrayBuffer missing');
  var buf = new SharedArrayBuffer(8);
  if (!buf) throw new Error('SharedArrayBuffer failed');
  if (buf.byteLength !== 8) throw new Error('SharedArrayBuffer byteLength mismatch');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
