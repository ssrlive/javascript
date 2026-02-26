// feature probe for 'Symbol.search'
try {
  if (typeof Symbol === 'function' && typeof Symbol.search === 'symbol') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
