// feature probe for 'Int8Array'
try {
  var a = new Int8Array([1, -1]);
  if (a.length !== 2 || a[1] !== -1) throw new Error('Int8Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
