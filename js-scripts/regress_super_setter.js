// Regression test for super setter invoking prototype setter with original receiver
// Should not panic with RefCell borrow errors
class Base {
  set x(v) {
    // Setter mutates receiver
    this._x = v;
  }
}
class Derived extends Base {
  testSet() {
    super.x = 42; // should invoke Base.prototype.x setter with receiver = instance
  }
}
const d = new Derived();
d.testSet();
if (d._x !== 42) throw new Error('super setter did not set receiver property');
console.log('ok');
