// feature probe for 'Uint8Array'
try {
  var a = new Uint8Array([1, 255]);
  if (a.length !== 2 || a[1] !== 255) throw new Error('Uint8Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
