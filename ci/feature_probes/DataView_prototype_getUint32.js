// feature probe for 'DataView.prototype.getUint32'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getUint32 !== 'function') throw new Error('getUint32 missing');
  dv.getUint32(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
