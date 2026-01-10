"use strict";

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
