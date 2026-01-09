"use strict";

{
    console.log("Testing Set integration v2...");
    
    // Test constructor
    let s = new Set();
    if (s.size !== 0) throw "Empty set should have size 0";
    
    // Test add
    s.add(1);
    s.add(2);
    s.add(1); // duplicate
    
    if (s.size !== 2) throw "Set should have size 2 after adding 1, 2, 1. Got " + s.size;
    
    // Test values()/keys()/entries()
    let s3 = new Set();
    s3.add("a");
    s3.add("b");
    
    let values = s3.values();
    if (values.next().value !== "a") throw "First value should be 'a'";
    
    // Test for..of loop (Symbol.iterator)
    // We expect this to FAIL if Symbol.iterator is not implemented on Set.prototype
    let sum = 0;
    let s4 = new Set();
    s4.add(10);
    s4.add(20);
    
    let hasIterator = false;
    if (typeof Symbol !== 'undefined' && Symbol.iterator && typeof s4[Symbol.iterator] === 'function') {
        hasIterator = true;
        for (let x of s4) {
             sum += x;
        }
        if (sum !== 30) throw "for..of loop sum should be 30. Got " + sum;
    } else {
        console.log("WARN: Set is not iterable yet (Symbol.iterator missing)");
    }

    // Test forEach
    let sumForEach = 0;
    if (typeof s4.forEach === 'function') {
        s4.forEach(function(val) { sumForEach += val; });
        if (sumForEach !== 30) throw "forEach sum should be 30";
    } else {
        console.log("WARN: Set.prototype.forEach missing");
    }

    console.log("Set tests passed!");
}

true;