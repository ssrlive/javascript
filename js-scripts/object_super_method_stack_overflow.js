function assert(b) {
    if (!b) {
        throw new Error("Bad assertion");
    }
}

class X {
    toString() {
        return "X toString";
    }
}
class Y extends X {
    toString() {
        return "Y " + super.toString();
    }
}
class Z extends Y {
    toString() {
        return "Z " + super.toString();
    }
}
let obj = new Z();
console.log(obj.toString()); // Expected output: "Z Y X toString"

assert(obj.toString() === "Z Y X toString");

class A extends Z {
    toString() {
        return "A " + super.toString();
    }
}
let objA = new A();
console.log(objA.toString()); // Expected output: "A Z Y X toString"
assert(objA.toString() === "A Z Y X toString");

console.log("All assertions passed.");
