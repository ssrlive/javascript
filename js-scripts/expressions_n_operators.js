"use strict";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "断言失败");
  }
}

var a1 = true && true; // t && t returns true
var a2 = true && false; // t && f returns false
var a3 = false && true; // f && t returns false
var a4 = false && 3 == 4; // f && f returns false
var a5 = "Cat" && "Dog"; // t && t returns Dog
var a6 = false && "Cat"; // f && t returns false
var a7 = "Cat" && false; // t && f returns false

console.log(a1, a2, a3, a4, a5, a6, a7);

var o1 = true || true; // t || t returns true
var o2 = false || true; // f || t returns true
var o3 = true || false; // t || f returns true
var o4 = false || 3 == 4; // f || f returns false
var o5 = "Cat" || "Dog"; // t || t returns Cat
var o6 = false || "Cat"; // f || t returns Cat
var o7 = "Cat" || false; // t || f returns Cat

console.log(o1, o2, o3, o4, o5, o6, o7);


var n1 = !true; // !t returns false
var n2 = !false; // !f returns true
var n3 = !"Cat"; // !t returns false

console.log(n1, n2, n3);

var myString = "alpha";
myString += "bet"; // 返回 "alphabet"

var x = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
var a = [x, x, x, x, x];

for (var i = 0, j = 9; i <= j; i++, j--)
  console.log("a[" + i + "][" + j + "]= " + a[i][j]);


var yy = 43;
let myobj = new Number();
myobj.h = 4; // create property h
var res2, res3, res4, res5;

try {
  res2 = delete yy; // throws SyntaxError in strict mode
  assert(false, "delete yy did not throw");
} catch (e) {
  console.log("Caught expected error for delete yy:", e.message);
  assert(e instanceof SyntaxError, "delete yy threw wrong type of error");
  res2 = false;
}

try {
  res3 = delete Math.PI; // throws TypeError in strict mode
} catch (e) {
  console.log("Caught expected error for delete Math.PI:", e.message);
  assert(e instanceof TypeError, "delete Math.PI threw wrong type of error");
  res3 = false;
}

res4 = delete myobj.h; // returns true (configurable)

try {
  res5 = delete myobj; // throws SyntaxError in strict mode
  assert(false, "delete myobj did not throw");
} catch (e) {
  console.log("Caught expected error for delete myobj:", e.message);
  assert(e instanceof SyntaxError, "delete myobj threw wrong type of error");
  res5 = false;
}

console.log(res2, res3, res4, res5);

assert(!res2, "delete yy should be false or throw");
assert(!res3, "delete Math.PI should be false or throw");
assert(res4, "delete myobj.h failed");
assert(!res5, "delete myobj should be false or throw");

console.log("xx =", typeof xx !== "undefined" ? xx : "undefined");
console.log("yy =", typeof yy !== "undefined" ? yy : "undefined");
console.log("Math.PI =", Math.PI);
console.log("myobj =", typeof myobj !== "undefined" ? myobj : "undefined");


var trees = new Array("redwood", "bay", "cedar", "oak", "maple");
delete trees[3];
if (3 in trees) {
  // 不会被执行
  console.log("删除 trees[3] 失败");
} else {
  console.log("删除 trees[3] 成功");
}
assert(!(3 in trees), "删除 trees[3] 失败");
console.log("trees length =", trees.length); // 5

var trees = new Array("redwood", "bay", "cedar", "oak", "maple");
trees[3] = undefined;
assert(3 in trees, "trees[3] 设置为 undefined 失败");
if (3 in trees) {
  // this gets executed（会被执行）
  console.log("trees[3] 设置为 undefined 成功");
} else {
  // 不会被执行
  console.log("trees[3] 设置为 undefined 失败");
}
console.log("trees length =", trees.length); // 5


var myFun = new Function("5 + 2");
typeof myFun; // returns "function"
assert(typeof myFun === "function", "typeof myFun should be 'function'");
console.log("myFun() =", myFun());

console.log("Implicit return:", new Function("5 + 2")());
console.log("Explicit return:", new Function("return 5 + 2")());
console.log("Eval:", eval("5 + 2"));

var shape = "round";
typeof shape; // returns "string"
assert(typeof shape === "string", "typeof shape should be 'string'");

var size = 1;
typeof size; // returns "number"
assert(typeof size === "number", "typeof size should be 'number'");

var today = new Date();
typeof today; // returns "object"
assert(typeof today === "object", "typeof today should be 'object'");

typeof dontExist; // returns "undefined"
assert(typeof dontExist === "undefined", "typeof dontExist should be 'undefined'");

typeof true; // returns "boolean"
typeof null; // returns "object"
assert(typeof true === "boolean", "typeof true should be 'boolean'");
assert(typeof null === "object", "typeof null should be 'object'");

typeof 62; // returns "number"
typeof "Hello world"; // returns "string"
assert(typeof 62 === "number", "typeof 62 should be 'number'");
assert(typeof "Hello world" === "string", "typeof 'Hello world' should be 'string'");

typeof Math.LN2; // returns "number"
assert(typeof Math.LN2 === "number", "typeof Math.LN2 should be 'number'");

typeof eval; // returns "function"
typeof parseInt; // returns "function"
typeof shape.split; // returns "function"
console.log("typeof eval =", typeof eval);
console.log("typeof parseInt =", typeof parseInt);
console.log("typeof shape.split =", typeof shape.split);
assert(typeof eval === "function", "typeof eval should be 'function'");
assert(typeof parseInt === "function", "typeof parseInt should be 'function'");
assert(typeof shape.split === "function", "typeof shape.split should be 'function'");


typeof Date; // returns "function"
typeof Function; // returns "function"
typeof Math; // returns "object"
console.log("typeof Math =", typeof Math);
typeof String; // returns "function"
console.log("typeof String =", typeof String);
assert(typeof Date === "function", "typeof Date should be 'function'");
assert(typeof Function === "function", "typeof Function should be 'function'");
assert(typeof Math === "object", "typeof Math should be 'object'");
assert(typeof String === "function", "typeof String should be 'function'");


void 9+5;
void (9+5);
void "expression" * 90;
console.log("void operator tests passed." * 90);
void ("expression");


// Arrays
var trees = new Array("redwood", "bay", "cedar", "oak", "maple");
assert(0 in trees, "0 in trees should be true");
assert(3 in trees, "3 in trees should be true");
assert(!(6 in trees), "6 in trees should be false");
assert(!("bay" in trees), "'bay' in trees should be false (you must specify the index number, not the value at that index)");
assert("length" in trees, "'length' in trees should be true (length is an Array property)");

// Predefined objects
assert("PI" in Math, "'PI' in Math should be true");
var myString = new String("coral");
assert("length" in myString, "'length' in myString should be true");

// Custom objects
var mycar = { make: "Honda", model: "Accord", year: 1998 };
assert("make" in mycar, "'make' in mycar should be true");
assert("model" in mycar, "'model' in mycar should be true");
assert("year" in mycar, "'year' in mycar should be true");

var theDay = new Date(1995, 12, 17);
assert(theDay instanceof Date, "theDay should be instance of Date");


var a = 1;
var b = 2;
var c = 3;

// 默认优先级
a + b * c; // 7
assert(a + b * c === 7, "a + b * c should be 7");

// 使用括号改变优先级
(a + b) * c; // 9
assert((a + b) * c === 9, "(a + b) * c should be 9");

// 等价表达式
a * c + b * c; // 9
assert(a * c + b * c === 9, "a * c + b * c should be 9");


var v1 = [ 1, 2, 3 ].map(i => i*i);
console.log(v1);
assert(Array.isArray(v1), "v1 should be an array");
assert(JSON.stringify(v1) === JSON.stringify([1, 4, 9]), "v1 should be [1, 4, 9]"); // [ 1, 4, 9 ]

var abc = [ "A", "B", "C" ];
abc = abc.map(letters => letters.toLowerCase());
console.log(abc);
assert(Array.isArray(abc), "abc should be an array");
assert(JSON.stringify(abc) === JSON.stringify([ "a", "b", "c" ]), "abc should be [ 'a', 'b', 'c' ]"); // [ "a", "b", "c" ]

var object = {};

console.log(object.property); // undefined
object.property;
object["property"];

var maybeObject = null;
maybeObject?.property;
maybeObject?.[property];

var maybeFunction;
maybeFunction?.();

class ObjectType {
  constructor(param1, param2, /* …, */ paramN) {
    this.param1 = param1;
    this.param2 = param2;
    // …
    this.paramN = paramN;
  }
}

const objectName = new ObjectType("param1", "param2", /* …, */ "paramN");

console.log(objectName.param1);
console.log(objectName.param2);

{
  console.log("==== Test for-in loop ====");
  function fn(x) {
    let a = [];
    for (let p in x) {
      a.push(function () { return p; });
    }
    let k = 0;
    for (let q in x) {
      assert(q == a[k](), "for-in loop variable mismatch: " + q + " != " + a[k]());
      ++k;
    }
  }
  fn({a : [0], b : 1, c : {v : 1}, get d() {}, set e(x) {}});
}

console.log("All tests passed.");
