// feature probe for 'iterator-helpers'
// Tests Iterator.from and Iterator.prototype helper methods (map, filter, take, etc.)
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  if (typeof Iterator.from !== 'function') throw new Error('Iterator.from missing');
  var iter = Iterator.from([1, 2, 3]);
  if (typeof iter.map !== 'function') throw new Error('Iterator.prototype.map missing');
  if (typeof iter.filter !== 'function') throw new Error('Iterator.prototype.filter missing');
  if (typeof iter.take !== 'function') throw new Error('Iterator.prototype.take missing');
  console.log('OK');
} catch (e) {
  console.log('NO:', e.message);
}
