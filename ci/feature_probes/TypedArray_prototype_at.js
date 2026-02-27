// feature probe for 'TypedArray.prototype.at'
try {
  var a = new Uint8Array([10, 20, 30]);
  if (typeof a.at !== 'function') throw new Error('at missing');
  if (a.at(-1) !== 30) throw new Error('at wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
