// feature probe for 'regexp-unicode-property-escapes'
try {
  var re = new RegExp('\\p{Letter}', 'u');
  if (re.test('a') && !re.test('1')) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
