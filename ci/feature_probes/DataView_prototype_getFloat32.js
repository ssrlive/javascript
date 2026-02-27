// feature probe for 'DataView.prototype.getFloat32'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getFloat32 !== 'function') throw new Error('getFloat32 missing');
  dv.getFloat32(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
