try {
  var ab = new ArrayBuffer(1, { maxByteLength: 2 });
  if (typeof ab.resize !== 'function') throw new Error('no resize');
  ab.resize(2);
  var sab = new SharedArrayBuffer(1, { maxByteLength: 2 });
  if (typeof sab.grow !== 'function') throw new Error('no grow');
  sab.grow(2);
  console.log('OK');
} catch (e) { console.log('NO'); }
