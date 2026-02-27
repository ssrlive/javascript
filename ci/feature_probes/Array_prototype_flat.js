// feature probe for 'Array.prototype.flat'
try {
  if (typeof [].flat !== 'function') throw new Error('flat missing');
  var r = [1,[2,[3]]].flat();
  if (r.length !== 3 || r[2][0] !== 3) throw new Error('flat wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
