try {
  function f(a, b, ...rest) { return rest.length; }
  if (f(1, 2, 3, 4, 5) === 3) console.log("OK");
} catch (e) {
  // not supported
}
