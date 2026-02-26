// feature probe for 'Symbol.matchAll'
try {
  if (typeof Symbol === 'function' && typeof Symbol.matchAll === 'symbol') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
