"use strict";

function assert(mustBeTrue, message) {
  if (!mustBeTrue) {
    throw new Error(message || "Assertion failed");
  }
}

function MyError(message) {
  this.message = message || "";
}

MyError.prototype.toString = function() { return "MyError: " + this.message; };

try {
  throw new MyError('dbg-test');
} catch(e) {
  assert(e.constructor === MyError, `Expected thrown error's constructor to be MyError but got ${e.constructor}`);
  assert(e.constructor && e.constructor.name === 'MyError', `Expected thrown error's constructor name to be 'MyError' but got '${e.constructor && e.constructor.name}'`);
  assert(e.message === 'dbg-test', `Expected thrown error's message to be 'dbg-test' but got '${e.message}'`);
  assert(e.toString() === 'MyError: dbg-test', `Expected thrown error's toString() to be 'MyError: dbg-test' but got '${e.toString()}'`);
}

{
  function Person(name){ this.name = name; }
  Person.prototype.greet = function(){ return `hi ${this.name}`; };

  const p = new Person('A');

  assert(p.greet() === 'hi A', `Expected p.greet() to be 'hi A' but got '${p.greet()}'`);
  assert('greet' in p, `Expected 'greet' in p to be true but got false`);
  assert(p.hasOwnProperty('greet') === false, `Expected p.hasOwnProperty('greet') to be false but got true`);
  assert(Object.prototype.hasOwnProperty.call(p, 'greet') === false, `Expected Object.prototype.hasOwnProperty.call(p, 'greet') to be false but got true`);
  assert(p.hasOwnProperty('name') === true, `Expected p.hasOwnProperty('name') to be true but got false`);
  assert(Object.prototype.hasOwnProperty.call(p, 'name') === true, `Expected Object.prototype.hasOwnProperty.call(p, 'name') to be true but got false`);
}

{
  var xFn;
  xFn = function() {};
  // console.log(xFn.name);
  assert(xFn.name === 'xFn', `Expected function name to be 'xFn' but got '${xFn.name}'`);

  var d = Object.getOwnPropertyDescriptor(xFn, 'name');
  assert(d.value === 'xFn', `Expected descriptor value to be 'xFn' but got '${d.value}'`);
  assert(d.writable === false, `Expected descriptor writable to be false but got '${d.writable}'`);
  assert(d.enumerable === false, `Expected descriptor enumerable to be false but got '${d.enumerable}'`);
  assert(d.configurable === true, `Expected descriptor configurable to be true but got '${d.configurable}'`);
}

{
  var arrow;
  arrow = () => {};
  var d = Object.getOwnPropertyDescriptor(arrow, 'name');
  assert(d.value === 'arrow', `Expected descriptor value to be 'arrow' but got '${d.value}'`);
  assert(d.writable === false, `Expected descriptor writable to be false but got '${d.writable}'`);
  assert(d.enumerable === false, `Expected descriptor enumerable to be false but got '${d.enumerable}'`);
  assert(d.configurable === true, `Expected descriptor configurable to be true but got '${d.configurable}'`);
}

{
  function foo() {};
  Object.defineProperty(foo.prototype, "bar", {value: "unwritable"}); 

  var o = new foo();
  try {
    o.bar = "overridden";
    assert(false, "Expected assignment to throw TypeError but no exception was thrown");
  } catch (e) {
    assert(e instanceof TypeError, `Expected thrown error to be TypeError but got ${e}`);
  }

  assert(o.bar === "unwritable", `Expected o.bar to be 'unwritable' but got '${o.bar}'`);
}

{
    console.log('=== 13.15.2 - SuperProperty Assignments on classes with null prototype ===');
    var count_13_15_2 = 0;
    class C_13_15_2 {
        static m() {
            super.x = count_13_15_2 += 1;
        }
    }
    Object.setPrototypeOf(C_13_15_2, null);
    try {
        C_13_15_2.m();
        throw new Error('Expected to throw, but no exception was thrown');
    } catch (e) {
        if (!(e instanceof TypeError)) {
            console.log(e);
            throw new Error('Expected TypeError to be thrown but got ' + typeof e);
        }
    }
    assert(count_13_15_2 === 1, 'The value of count_13_15_2 is expected to be 1');
}

{
    console.log('=== identifierreference await ===');
    try {
        var await = 0;
        await = 199;

        var async = 30;
        async = 256;
    } catch (e) {
        (function(){
            console.log('msg', "Somehow caught an exception:", e);
        })();
        throw e;
    }
}
