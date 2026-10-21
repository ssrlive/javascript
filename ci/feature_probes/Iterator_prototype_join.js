// feature probe for 'Iterator.prototype.join'
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  if (Iterator.from([1, null, 3]).join('|') !== '1||3') throw new Error('join result incorrect');
  console.log('OK');
} catch (_e) {
  console.log('NO');
}
