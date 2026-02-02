// Probe for Symbol.asyncIterator support
try {
  if (typeof Symbol === 'function' && typeof Symbol.asyncIterator === 'symbol') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
