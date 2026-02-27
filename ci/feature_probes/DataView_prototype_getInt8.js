// feature probe for 'DataView.prototype.getInt8'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getInt8 !== 'function') throw new Error('getInt8 missing');
  dv.getInt8(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
