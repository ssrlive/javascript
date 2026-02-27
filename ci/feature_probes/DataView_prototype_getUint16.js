// feature probe for 'DataView.prototype.getUint16'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getUint16 !== 'function') throw new Error('getUint16 missing');
  dv.getUint16(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
