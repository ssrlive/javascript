// feature probe for 'DataView.prototype.getInt32'
try {
  var ab = new ArrayBuffer(8);
  var dv = new DataView(ab);
  if (typeof dv.getInt32 !== 'function') throw new Error('getInt32 missing');
  dv.getInt32(0);
  console.log('OK');
} catch (e) { console.log('NO'); }
