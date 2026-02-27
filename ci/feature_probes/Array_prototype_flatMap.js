// feature probe for 'Array.prototype.flatMap'
try {
  if (typeof [].flatMap !== 'function') throw new Error('flatMap missing');
  var r = [1,2].flatMap(function(x){return [x,x*2];});
  if (r.length !== 4 || r[2] !== 2) throw new Error('flatMap wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
