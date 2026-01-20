"use strict";

/*
// Non-strict mode test for arguments.callee

function testCallee() {
    console.log("In testCallee, arguments.callee =", arguments.callee);
    if (arguments.callee !== testCallee) {
        throw new Error("arguments.callee is not the function itself");
    }
    return "ok";
}

console.log("=== Testing arguments.callee property ===");
{
    // Normal call
    let v = testCallee();
    console.log("Normal call: pass", v);

    // Call via .call
    testCallee.call(null);
    console.log("Call via .call: pass");

    // Call via .apply
    testCallee.apply(null, []);
    console.log("Call via .apply: pass");

    // Closure call
    var f = function() {
        if (arguments.callee !== f) {
            throw new Error("arguments.callee (closure) is not the function itself");
        }
        console.log("In closure f, arguments.callee =", arguments.callee);
    };
    f();
    console.log("Closure call: pass");
    f.call({});
    console.log("Closure .call: pass");
    f.apply({}, []);
    console.log("Closure .apply: pass");
}
// */

console.log("=== Checking existence of Function.prototype.call, apply, bind: ===");
try {
    console.log("Function.prototype.call: " + Function.prototype.call);
    console.log("Function.prototype.apply: " + Function.prototype.apply);
    console.log("Function.prototype.bind: " + Function.prototype.bind);
    
    var f = function() {};
    console.log("f.call: " + f.call);
    console.log("f.apply: " + f.apply);

} catch (e) {
    console.log("Error: " + e);
}

{
    console.log("=== Testing that accessing arguments.callee throws TypeError in strict mode ===");
    try {
        arguments.callee;
        throw new Error("Accessing arguments.callee did not throw");
    } catch (e) {
        console.log(e);
        if (!(e instanceof TypeError)) {
            throw new Error('Expected a TypeError, but got: ' + e);
        }
    }
}

{
    console.log("=== Testing that arguments.length is writable ===");

    let str = "something different";

    function f1(){
        arguments.length = str;
        return arguments;
    }

    try{
        if(f1().length !== str){
            throw new Error("#1: A property length have attribute { ReadOnly }");
        }
    }
    catch(e){
        console.log(e);
        throw new Error("#1: arguments object don't exists");
    }
}

return true;