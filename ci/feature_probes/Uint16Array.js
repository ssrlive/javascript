// feature probe for 'Uint16Array'
try {
  var a = new Uint16Array([1, 0xFFFF]);
  if (a.length !== 2 || a[1] !== 0xFFFF) throw new Error('Uint16Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
