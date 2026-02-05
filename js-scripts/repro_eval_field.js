
var executed = false;
var res = eval('executed = true; () => 1;');
console.log("res type: " + typeof res);
if (typeof res === 'function') {
    console.log("res() = " + res());
} else {
    console.log("res is " + res);
}

class C {
  x = eval('executed = true; () => 42;');
}

var c = new C();
console.log("c.x type: " + typeof c.x);
if (typeof c.x === 'function') {
    console.log("c.x() = " + c.x());
} else {
    console.log("c.x is " + c.x);
}
