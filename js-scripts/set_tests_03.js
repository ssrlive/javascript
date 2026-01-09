"use strict";

{
    console.log("Testing Set integration v3...");
    
    // Test forEach
    let s4 = new Set();
    s4.add(10);
    s4.add(20);
    
    let sumForEach = 0;
    if (typeof s4.forEach === 'function') {
        s4.forEach(function(val) { 
            sumForEach = sumForEach + val; 
        });
        if (sumForEach !== 30) throw "forEach sum should be 30. Got " + sumForEach;
    } else {
        throw "Set.prototype.forEach missing";
    }
    
    // Test forEach arguments (value, value2, set)
    let count = 0;
    s4.forEach(function(v1, v2, s) {
        if (v1 !== v2) throw "forEach value match failed";
        if (s !== s4) throw "forEach set arg failed";
        count = count + 1;
    });
    if (count !== 2) throw "forEach count wrong";

    console.log("Set tests v3 passed!");
}

true;
