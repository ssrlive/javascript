// feature probe for 'Int16Array'
try {
  var a = new Int16Array([1, -1]);
  if (a.length !== 2 || a[1] !== -1) throw new Error('Int16Array wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
