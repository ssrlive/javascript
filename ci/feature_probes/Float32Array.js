// feature probe for 'Float32Array'
try {
  if (typeof Float32Array === 'undefined') throw new Error('Float32Array missing');
  var arr = new Float32Array(4);
  if (arr.length !== 4) throw new Error('Float32Array length mismatch');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
