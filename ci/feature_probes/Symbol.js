try {
  if (typeof Symbol === 'function') {
    Symbol('x');
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
