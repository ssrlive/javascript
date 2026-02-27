// feature probe for 'DataView.prototype.setUint8'
try {
  var ab = new ArrayBuffer(4);
  var dv = new DataView(ab);
  dv.setUint8(0, 42);
  if (dv.getUint8(0) !== 42) throw new Error('setUint8 failed');
  console.log('OK');
} catch (e) { console.log('NO'); }
