// feature probe for 'immutable-arraybuffer'
try {
  var ab = new ArrayBuffer(4);
  var iab = ab.transferToImmutable();
  if (iab.immutable !== true) throw new Error('immutable not true');
  if (ab.detached !== true) throw new Error('original not detached');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
