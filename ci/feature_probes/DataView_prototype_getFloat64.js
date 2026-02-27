// feature probe for 'DataView.prototype.getFloat64'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getFloat64 !== 'function') throw new Error('getFloat64 missing');
  dv.getFloat64(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
