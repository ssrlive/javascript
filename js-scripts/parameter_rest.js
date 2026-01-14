const assert = (actual, expected, message) => {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        console.log(`FAIL: ${message} - Expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
        throw new Error(`Assertion failed: ${message}`);
    } else {
        console.log(`PASS: ${message}`);
    }
};

// 1. Regular function
function regular(a, ...rest) {
    return [a, rest];
}
assert(regular(1, 2, 3), [1, [2, 3]], "Regular function with rest");

// 2. Arrow function
const arrow = (a, ...rest) => [a, rest];
assert(arrow(1, 2, 3), [1, [2, 3]], "Arrow function with rest");

// 4. Optional call
const obj = {
    method(a, ...rest) {
        return [a, rest];
    }
};
assert(obj?.method(1, 2, 3), [1, [2, 3]], "Optional call with rest");

// 5. Constructor
class MyClass {
    constructor(a, ...rest) {
        this.a = a;
        this.rest = rest;
    }
}
const instance = new MyClass(1, 2, 3);
assert(instance.a, 1, "Constructor param a");
assert(instance.rest, [2, 3], "Constructor param rest");

// 6. Spread in constructor
const args = [2, 3];
const instance2 = new MyClass(1, ...args);
assert(instance2.a, 1, "Constructor with spread param a");
assert(instance2.rest, [2, 3], "Constructor with spread param rest");


const fn = (a, ...args) => {
    assert(a, 1, "First argument a");
    assert(args, [2, 3, 4], "Rest arguments args");
    console.log("args is Array:", Array.isArray(args));
    console.log("args length:", args.length);
    console.log("args:", args);
};
fn(1, 2, 3, 4);

{
    const todos = ["学习 JavaScript", "学习 Web API", "构建网站", "利润！"];
    const progress = { javascript: 20, html: 50, css: "10" };
    console.log("\n我需要做:\n%o\n当前进度为: %o\n", todos, progress);
}

console.log("All tests passed!");
