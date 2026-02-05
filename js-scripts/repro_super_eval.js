
var A = class {
    constructor() { } 
}
A.prototype.x = "superX";

var C = class extends A {
  x = eval('() => super.x;');
};

try {
    var c = new C();
    console.log("c.x type: " + typeof c.x);
    console.log("c.x() result: " + c.x());
} catch (e) {
    console.log("Error: " + e);
    if (e.stack) console.log(e.stack);
}
