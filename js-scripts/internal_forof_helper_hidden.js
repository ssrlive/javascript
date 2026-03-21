function assert(condition, message) {
  if (!condition) {
    throw new Error(message || 'Assertion failed');
  }
}

assert(typeof __forOfValues === 'undefined', '__forOfValues should not be visible to scripts');
assert(
  !Object.prototype.hasOwnProperty.call(globalThis, '__forOfValues'),
  'globalThis should not expose __forOfValues'
);
