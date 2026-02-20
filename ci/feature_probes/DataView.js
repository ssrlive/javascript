// feature probe for 'DataView'
try {
  if (typeof DataView === 'undefined') throw new Error('DataView missing');
  var view = new DataView(new ArrayBuffer(8));
  if (!view || typeof view !== 'object') throw new Error('DataView construction failed');
  if (view.byteLength !== 8) throw new Error('DataView byteLength mismatch');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
