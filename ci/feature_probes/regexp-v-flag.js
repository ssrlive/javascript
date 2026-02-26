// feature probe for 'regexp-v-flag'
try {
  var re = new RegExp('[\\p{ASCII}]', 'v');
  if (re.test('a') && !re.test('\u0100')) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
