// Feature probe: well-formed-json-stringify
// JSON.stringify should escape lone surrogates as \uXXXX
try {
  var s = JSON.stringify('\uD800');
  if (s !== '"\\ud800"') throw new Error('lone high surrogate not escaped: ' + s);
  var s2 = JSON.stringify('\uDFFF');
  if (s2 !== '"\\udfff"') throw new Error('lone low surrogate not escaped: ' + s2);
  console.log('OK');
} catch(e) {
  console.log('NO');
}
