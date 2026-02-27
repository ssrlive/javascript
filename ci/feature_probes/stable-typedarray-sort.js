// feature probe for 'stable-typedarray-sort'
try {
  var a = new Int32Array([3, 1, 2]);
  a.sort();
  if (a[0] !== 1 || a[1] !== 2 || a[2] !== 3) throw new Error('sort wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
