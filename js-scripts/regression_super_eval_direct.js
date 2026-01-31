// Regression test: direct eval of super.property in non-arrow method
// should return undefined when parent lacks the property, and reflect later prototype changes.
var superProp = null;
var o = {
  test262: null,
  method() {
    superProp = eval('super.test262;');
  }
};

o.method();
if (!(typeof superProp === 'undefined')) throw new Error('Expected initial superProp to be undefined, got: ' + superProp);
Object.setPrototypeOf(o, { test262: 262 });
o.method();
if (superProp !== 262) throw new Error('Expected superProp to be 262 after prototype change, got: ' + superProp);
console.log('OK');
