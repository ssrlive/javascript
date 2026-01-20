"use strict";

function assert(mustBeTrue, message) {
    if (!mustBeTrue) {
        throw new Error(message || "Assertion failed");
    }
}

{
    var inventory = [
        { name: "asparagus", type: "vegetables", quantity: 5 },
        { name: "bananas", type: "fruit", quantity: 0 },
        { name: "goat", type: "meat", quantity: 23 },
        { name: "cherries", type: "fruit", quantity: 5 },
        { name: "fish", type: "meat", quantity: 22 },
    ];

    var result = Object.groupBy(inventory, (item) => item.type);

    console.log(Object.keys(result));
    // expected: ["vegetables", "fruit", "meat"] (order may vary?)
    console.log(result.vegetables.length); // 1
    console.log(result.fruit.length); // 2
    console.log(result.meat.length); // 2
}

{
    var obj = {a:1, b:2};
    console.log(Object.keys(obj));
    console.log(Object.values(obj));
    console.log(Object.hasOwn(obj, 'a'));
    var o = Object.create(null);
    console.log(Object.getPrototypeOf(o) === null);
}

{
    let i1 = {type: "veg"};
    let i2 = {type: "fruit"};
    let inventory = [i1, i2];

    let result = Object.groupBy(inventory, (item) => item.type);
    console.log(result.veg.length);
    console.log(result.fruit.length);
}

{
    console.log("==== Test non-extensible object ====");

    var _8_7_2_5 = {};
    Object.preventExtensions(_8_7_2_5);
    try {
        _8_7_2_5.a = 10;
        assert(false, 'Assigning a property to a non-extensible object should throw a TypeError');
    } catch (e) {
        assert(e instanceof TypeError);
    }
}

{
    console.log("==== Test non-writable property ====");

    var _8_7_2_3 = {};
    Object.defineProperty(_8_7_2_3, "b", {
        writable: false
    });
    try {
        _8_7_2_3.b = 11;
        assert(false, 'Assigning to a non-writable property should throw a TypeError');
    } catch (e) {
        assert(e instanceof TypeError);
    }
}

{
    console.log("==== Test changing __proto__ of non-extensible object ====");
    let x = Object.preventExtensions({});
    let y = {};
    try {
        x.__proto__ = y;
        assert(false, 'Changing __proto__ of a non-extensible object should throw a TypeError');
    } catch (err) {
        // As far as this test is concerned, we allow the above assignment
        // to fail. This failure does violate the spec and should probably
        // be tested separately.
        console.log("Caught error while changing __proto__: " + err);
        assert(err instanceof TypeError);
    }
    assert(Object.getPrototypeOf(x) === Object.prototype, "Prototype of non-extensible object should not be mutated");
}

{
    console.log("==== Test Object constructor ====");
    var objInstance = new Object;
    console.log(objInstance.constructor);
    assert(objInstance.constructor === Object, 'objInstance.constructor should be Object');

    var numInstance = new Number;
    console.log(numInstance.constructor);
    assert(numInstance.constructor === Number, 'numInstance.constructor should be Number');
}

return true;
