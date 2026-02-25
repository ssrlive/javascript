// feature probe for 'Symbol.match'
try {
  if (typeof Symbol !== 'function' || typeof Symbol.match !== 'symbol') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
