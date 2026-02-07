try {
  if (typeof Symbol === 'function' && typeof Symbol.hasInstance === 'symbol') {
    function F() {}
    F[Symbol.hasInstance] = function() { return true; };
    if (1 instanceof F) {
      console.log('OK');
    } else {
      console.log('NO');
    }
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
