// Feature probe: Symbol.isConcatSpreadable
try {
  if (typeof Symbol.isConcatSpreadable !== 'symbol') throw new Error('missing');
  var obj = { length: 2, 0: 'a', 1: 'b', [Symbol.isConcatSpreadable]: true };
  var result = [].concat(obj);
  if (result.length !== 2 || result[0] !== 'a' || result[1] !== 'b') throw new Error('bad concat');
  console.log('OK');
} catch(e) {
  console.log('NO');
}
