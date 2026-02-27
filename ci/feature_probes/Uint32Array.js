// feature probe for 'Uint32Array'
try {
  var a = new Uint32Array([1, 0xFFFFFFFF]);
  if (a.length !== 2 || a[1] !== 0xFFFFFFFF) throw new Error('Uint32Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
