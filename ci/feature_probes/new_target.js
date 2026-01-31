// feature probe for 'new.target' support
try {
  function F() {
    // access new.target to ensure parser supports feature
    if (typeof new.target === 'undefined') {
      // ok when called without new
    }
  }
  new F();
  console.log('OK');
} catch (e) {
  console.log('NO');
}
