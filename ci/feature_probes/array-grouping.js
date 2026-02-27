// feature probe for 'array-grouping'
try {
  if (typeof Object.groupBy !== 'function') throw new Error('Object.groupBy missing');
  var r = Object.groupBy([1,2,3], function(x){return x > 1 ? 'big' : 'small';});
  if (r.small.length !== 1 || r.big.length !== 2) throw new Error('groupBy wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
