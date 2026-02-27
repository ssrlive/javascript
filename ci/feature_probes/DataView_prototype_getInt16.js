// feature probe for 'DataView.prototype.getInt16'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getInt16 !== 'function') throw new Error('getInt16 missing');
  dv.getInt16(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
