try {
  if (typeof Symbol !== 'function' || typeof Symbol.hasInstance !== 'symbol') {
    throw new Error('missing Symbol.hasInstance');
  }

  if (typeof Function.prototype[Symbol.hasInstance] !== 'function') {
    throw new Error('missing Function.prototype[@@hasInstance]');
  }

  function F() {}
  var inst = new F();
  if (Function.prototype[Symbol.hasInstance].call(F, inst) !== true) {
    throw new Error('positive check failed');
  }
  if (Function.prototype[Symbol.hasInstance].call(F, {}) !== false) {
    throw new Error('negative check failed');
  }

  console.log('OK');
} catch (e) {
  console.log('NO');
}
