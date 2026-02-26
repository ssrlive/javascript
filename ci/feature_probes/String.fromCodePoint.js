// feature probe for 'String.fromCodePoint'
try {
  if (typeof String.fromCodePoint === 'function' && String.fromCodePoint(65) === 'A') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
