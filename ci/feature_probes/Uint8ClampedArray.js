// feature probe for 'Uint8ClampedArray'
try {
  var a = new Uint8ClampedArray([1, 300]);
  if (a.length !== 2 || a[1] !== 255) throw new Error('Uint8ClampedArray wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
