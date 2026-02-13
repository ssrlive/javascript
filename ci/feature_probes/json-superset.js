// feature probe for 'json-superset'
try {
  // Build source text containing raw U+2028 and U+2029 in string literals.
  var ls = eval("'" + String.fromCharCode(0x2028) + "'");
  var ps = eval("'" + String.fromCharCode(0x2029) + "'");
  if (ls === "\u2028" && ps === "\u2029") {
    console.log("OK");
  } else {
    console.log("UNSUPPORTED 1");
  }
} catch (e) {
  console.log("UNSUPPORTED 2");
}
