// feature probe for 'regexp-dotall'
try {
  var re = new RegExp('a.b', 's');
  if (re.dotAll === true && re.test('a\nb')) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
