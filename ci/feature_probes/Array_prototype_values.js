// feature probe for 'Array.prototype.values'
try {
  if (typeof [].values !== 'function') throw new Error('values missing');
  var it = [10, 20].values();
  var first = it.next();
  if (first.value !== 10) throw new Error('values wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
