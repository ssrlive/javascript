// feature probe for 'globalThis'
try {
  if (typeof globalThis !== 'undefined') {
    console.log('OK');
  }
} catch (e) {
  // unsupported
}
