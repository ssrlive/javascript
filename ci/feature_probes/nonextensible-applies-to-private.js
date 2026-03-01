// Feature probe: nonextensible-applies-to-private
// Adding private fields to non-extensible objects should throw TypeError
class Base {
  constructor() { Object.preventExtensions(this); }
}
class Sub extends Base {
  #x;
}
try {
  new Sub();
  throw new Error("should have thrown");
} catch (e) {
  if (!(e instanceof TypeError)) throw e;
}
console.log('OK');
