function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

function A() {
    this.name = "A instance";
}
A.b = function() {
    this.name = "A.b instance";
};
A.prototype.c = function() {
    return "method c called";
};

// Test 1: new A.b()
// Should be new (A.b)(), creating an instance of A.b
try {
    var o1 = new A.b();
    console.log("new A.b() result name: " + o1.name);
} catch (e) {
    console.log("new A.b() failed: " + e);
    assert(false, "new A.b() test failed" + e);
}

// Test 2: new A.b
// Should be new (A.b), creating an instance of A.b
try {
    var o2 = new A.b;
    if (o2 && o2.name) {
        console.log("new A.b result name: " + o2.name);
    } else {
        console.log("new A.b result is: " + o2);
    }
} catch (e) {
    console.log("new A.b failed: " + e);
    assert(false, "new A.b test failed" + e);
}

// Test 3: new A().c
// Should be (new A()).c -> "method c called" (property access)
try {
    var val = new A().c;
    console.log("new A().c result: " + val);
} catch (e) {
    console.log("new A().c failed: " + e);
    assert(false, "new A().c test failed" + e);
}
