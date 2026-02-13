// feature probe for 'Symbol.unscopables'
try {
  if (typeof Symbol === 'function' && typeof Symbol.unscopables !== 'undefined') {
    console.log('OK');
  }
} catch (e) {
  // unsupported
}
