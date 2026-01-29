try {
  if (typeof Symbol === 'function' && typeof Symbol.iterator !== 'undefined') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
