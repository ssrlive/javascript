// feature probe for 'proxy-missing-checks'
// Tests proxy internal method invariant checks added in later spec editions.
// Probe: defineProperty trap returning true for a non-configurable+writable target
// property with a non-writable descriptor must throw TypeError.
try {
  var target = {};
  Object.defineProperty(target, 'x', { value: 1, writable: true, configurable: false });
  var p = new Proxy(target, {
    defineProperty: function(t, prop, desc) { return true; }
  });
  try {
    Object.defineProperty(p, 'x', { writable: false });
    // If no error, the check is not implemented
    console.log('NO');
  } catch (e) {
    // TypeError expected — the check is implemented
    if (e instanceof TypeError) {
      console.log('OK');
    } else {
      console.log('NO');
    }
  }
} catch (e) {
  console.log('NO');
}
