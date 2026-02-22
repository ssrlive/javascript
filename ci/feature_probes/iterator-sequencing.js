// feature probe for 'iterator-sequencing'
// Tests Iterator.concat (TC39 iterator sequencing proposal)
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  if (typeof Iterator.concat !== 'function') throw new Error('Iterator.concat missing');
  var iter = Iterator.concat([1, 2], [3, 4]);
  var arr = iter.toArray ? iter.toArray() : Array.from(iter);
  if (arr.length !== 4) throw new Error('Iterator.concat failed');
  console.log('OK');
} catch (e) {
  console.log('NO:', e.message);
}
