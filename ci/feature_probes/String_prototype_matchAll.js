// feature probe for 'String.prototype.matchAll'
try {
  if (typeof ''.matchAll !== 'function') throw new Error('matchAll missing');
  var it = 'abab'.matchAll(/a/g);
  var first = it.next();
  if (first.done || first.value[0] !== 'a') throw new Error('matchAll wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
