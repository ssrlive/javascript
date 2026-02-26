// feature probe for 'Symbol.split'
try {
  if (typeof Symbol === 'function' && typeof Symbol.split === 'symbol') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
