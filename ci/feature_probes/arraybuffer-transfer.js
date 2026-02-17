// feature probe for 'arraybuffer-transfer'
try {
  if (typeof ArrayBuffer !== 'function') throw new Error('ArrayBuffer missing');
  if (typeof ArrayBuffer.prototype.transfer !== 'function') throw new Error('transfer missing');
  if (typeof ArrayBuffer.prototype.transferToFixedLength !== 'function') throw new Error('transferToFixedLength missing');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
