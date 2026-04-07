// Probe for Symbol.asyncIterator support
try {
  if (typeof Symbol === 'function' && typeof Symbol.asyncIterator === 'symbol') {
    console.log('OK');
  } else {
    throw new Error('Symbol.asyncIterator not supported');
  }
} catch (e) {
  console.log('NO, reason: ' + e.message);
}
