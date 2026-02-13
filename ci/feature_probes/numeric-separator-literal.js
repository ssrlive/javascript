// feature probe for 'numeric-separator-literal'
try {
  var n = eval("1_000 + 0b1_0 + 0o1_0 + 0xF_F");
  var bi = eval("1_0n + 0b1_0n + 0o1_0n + 0xFn");
  if (n === (1000 + 2 + 8 + 255) && bi === 35n) {
    console.log("OK");
  }
} catch (e) {
  // unsupported
}
