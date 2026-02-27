// feature probe for 'Int32Array'
try {
  var a = new Int32Array([1, -1]);
  if (a.length !== 2 || a[1] !== -1) throw new Error('Int32Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
