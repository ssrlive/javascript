// Feature probe: uint8array-base64
try {
  if (typeof Uint8Array.fromBase64 !== 'function') throw new Error('fromBase64 missing');
  if (typeof Uint8Array.fromHex !== 'function') throw new Error('fromHex missing');
  var u = new Uint8Array([72, 101, 108, 108, 111]);
  if (typeof u.toBase64 !== 'function') throw new Error('toBase64 missing');
  if (typeof u.toHex !== 'function') throw new Error('toHex missing');
  if (typeof u.setFromBase64 !== 'function') throw new Error('setFromBase64 missing');
  if (typeof u.setFromHex !== 'function') throw new Error('setFromHex missing');
  console.log('OK');
} catch(e) {
  console.log('NO');
}
