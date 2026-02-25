// feature probe for 'Symbol.replace'
try {
  if (typeof Symbol !== 'function' || typeof Symbol.replace !== 'symbol') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
