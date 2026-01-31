function Test262Error() { Error.call(this); this.name = 'Test262Error'; }
Test262Error.prototype = Object.create(Error.prototype);
Test262Error.prototype.constructor = Test262Error;
let iter = {};
Object.defineProperty(iter, Symbol.iterator, {get: function(){ throw new Test262Error(); }});
function f([x]) {}
try {
  f(iter);
  console.log('NO_THROW');
} catch (e) {
  console.log('THREW', (e && e.name) || e);
}
