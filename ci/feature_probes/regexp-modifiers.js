// feature probe for 'regexp-modifiers'
try {
  var re = new RegExp('(?i:a)b');
  if (re.test('Ab') && !re.test('aB')) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
