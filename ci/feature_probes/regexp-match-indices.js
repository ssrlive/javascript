// feature probe for 'regexp-match-indices'
try {
  var re = new RegExp('a(b)', 'd');
  var m = re.exec('ab');
  if (m && m.indices && m.indices[0][0] === 0 && m.indices[0][1] === 2) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
