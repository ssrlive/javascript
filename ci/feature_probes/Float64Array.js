// feature probe for 'Float64Array'
try {
  if (typeof Float64Array === 'undefined') throw new Error('Float64Array missing');
  var arr = new Float64Array(4);
  if (arr.length !== 4) throw new Error('Float64Array length mismatch');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
