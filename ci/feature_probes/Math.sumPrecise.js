// Feature probe: Math.sumPrecise
try {
  if (typeof Math.sumPrecise !== 'function') throw new Error('sumPrecise missing');
  var result = Math.sumPrecise([1, 2, 3]);
  if (result !== 6) throw new Error('bad result: ' + result);
  console.log('OK');
} catch(e) {
  console.log('NO');
}
