// feature probe for 'error-stack-accessor'
try {
  var descriptor = Object.getOwnPropertyDescriptor(Error.prototype, 'stack');
  if (typeof descriptor.get !== 'function' || typeof descriptor.set !== 'function') {
    throw new Error('Error.prototype.stack accessor missing');
  }
  var error = new Error('probe');
  if (typeof error.stack !== 'string') throw new Error('Error stack missing');
  if (Object.prototype.hasOwnProperty.call(error, 'stack')) throw new Error('own stack property present');
  console.log('OK');
} catch (_e) {
  console.log('NO');
}
