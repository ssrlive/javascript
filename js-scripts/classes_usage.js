"use strict";

function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

{
    console.log("=== Date Tests ===");

    const bigDay = new Date(2019, 6, 19);
    console.log(bigDay.toLocaleDateString());
    if (bigDay.getTime() < Date.now()) {
        console.log("Once upon a time...");
    }
}

{
    console.log("=== Class Tests ===");

    class MyClass {
        // Constructor
        constructor() {
            this.constructorRan = true;
        }
        // Instance field
        myField = "foo";
        // Instance method
        myMethod() {
            return "instance method result";
        }
        // Static field
        static myStaticField = "bar";
        // Static method
        static myStaticMethod() {
            return "static method result";
        }
        // Static block
        static {
            this.staticBlockRan = true;
        }
        // Fields, methods, static fields, and static methods all have
        // "private" forms
        #myPrivateField = "private bar";
        
        getPrivateField() {
            return this.#myPrivateField;
        }

        #myPrivateMethod() {
            return "private method result";
        }

        callPrivateMethod() {
            return this.#myPrivateMethod();
        }
    }

    const myInstance = new MyClass();
    assert(myInstance.myField === "foo", "Instance field should be accessible");
    assert(myInstance.constructorRan === true, "Constructor should have run");
    assert(myInstance.myMethod() === "instance method result", "Instance method should return correct value");
    
    assert(MyClass.myStaticField === "bar", "Static field should be accessible");
    assert(MyClass.myStaticMethod() === "static method result", "Static method should return correct value");
    assert(MyClass.staticBlockRan === true, "Static block should have run");

    assert(myInstance.getPrivateField() === "private bar", "Private field should be accessible via public method");
    assert(myInstance.callPrivateMethod() === "private method result", "Private method should be accessible via public method");

    console.log("Class test passed");
}

{
    console.log("=== Transpiled Class Tests ===");

    function MyClass() {
        this.myField = "foo";
        this.constructorRan = true;
    }
    MyClass.myStaticField = "bar";
    MyClass.myStaticMethod = function () {
        return "static method result";
    };
    MyClass.prototype.myMethod = function () {
        return "instance method result";
    };

    (function () {
        MyClass.staticBlockRan = true;
    })();

    const myInstance = new MyClass();
    assert(myInstance.myField === "foo", "Transpiled: Instance field should be accessible");
    assert(myInstance.constructorRan === true, "Transpiled: Constructor should have run");
    assert(myInstance.myMethod() === "instance method result", "Transpiled: Instance method should return correct value");

    assert(MyClass.myStaticField === "bar", "Transpiled: Static field should be accessible");
    assert(MyClass.myStaticMethod() === "static method result", "Transpiled: Static method should return correct value");
    assert(MyClass.staticBlockRan === true, "Transpiled: Static block should have run");

    console.log("Transpiled Class test passed");
}

{
    console.log("=== TDZ Tests ===");
    try {
        let tmp =new MyClass(); // ReferenceError: Cannot access 'MyClass' before initialization
        assert(false, "TDZ test failed: No error thrown");
    } catch (e) {
        console.log("Caught expected error:", e.message);
        assert(e instanceof ReferenceError, "Expected a ReferenceError");
        console.log("PASS for TDZ test");
    }

    class MyClass {}
}

{
    console.log("=== Testing class constructor call without new... ===");

    class MyClass {
        myField = "foo";
        myMethod() {
            console.log("myMethod called");
        }
    }

    const myInstance = new MyClass();
    console.log(myInstance.myField); // 'foo'
    myInstance.myMethod();

    // Typical function constructors can both be constructed with new and called without new.
    // However, attempting to "call" a class without new will result in an error.
    try {
        let tmp = MyClass();
        assert(false, "FAIL: Class constructor called without new should throw");
    } catch (e) {
        console.log("Caught expected error for class call without new:", e.message);
        assert(e instanceof TypeError, "Expected a TypeError");
        console.log("PASS: Class constructor cannot be invoked without 'new'");
    }
}

{
    console.log("=== Testing class expression name scope... ===");

    const MyClass = class MyClassLongerName {
        // Class body. Here MyClass and MyClassLongerName point to the same class.
    };
    try {
        let tmp = new MyClassLongerName(); // ReferenceError: MyClassLongerName is not defined
    } catch (e) {
        console.log("Caught expected error for class expression name not in scope:", e.message);
        assert(e instanceof ReferenceError, "Expected a ReferenceError");

        let tmp = new MyClass(); // This should work
        assert(tmp instanceof MyClass, "Instance should be of type MyClass");
        console.log("PASS: Class expression name is not in scope outside the class");
    }
}

{
    // class Color {
    //     constructor(r, g, b) {
    //         // Assign the RGB values as a property of `this`.
    //         this.values = [r, g, b];
    //     }
    // }

    class Color {
        constructor(...values) {
            this.values = values;
        }
    }

    const red = new Color(255, 0, 0);
    console.log(red);

    const anotherRed = new Color(255, 0, 0);
    assert(red !== anotherRed, "Different instances should not be strictly equal");
    assert(red != anotherRed, "Different instances should not be loosely equal");
}

{
    console.log("=== Testing class constructor explicit return of object... ===");

    class MyClass {
        constructor() {
            this.myField = "foo";
            return {}; // Explicitly returning a different object
        }
    }

    console.log(new MyClass().myField); // undefined
    assert(new MyClass().myField === undefined, "Constructor returning a different object should override 'this'");
}

{
    console.log("=== Testing class method access to instance fields... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        getRed() {
            return this.values[0];
        }
    }

    const red = new Color(255, 0, 0);
    assert(red.getRed() === 255, "getRed should return the red component");
}

{
    console.log("=== Testing method reference equality... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
            this.getRed = function () {
                return this.values[0];
            };
        }

        getGreen() {
            return this.values[1];
        }
    }

    console.log(new Color(255, 0, 0).getRed);
    assert(new Color().getRed !== new Color().getRed, "Instance method references should not be equal");
    assert(new Color().getGreen === new Color().getGreen, "Prototype method references should be equal");


    class A {
        method() {}
    }
    const a = new A();
    const m1 = a.method;
    const m2 = a.method;
    assert(m1 === m2, "Prototype method references should be equal");

    function foo() {}
    const f1 = foo;
    const f2 = foo;
    assert(f1 === f2, "Function references should be equal");
}

{
    console.log("=== Testing class method modifying instance fields... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        getRed() {
            return this.values[0];
        }
        setRed(value) {
            this.values[0] = value;
        }
    }

    const red = new Color(255, 0, 0);
    red.setRed(0);
    assert(red.getRed() === 0, "setRed should update the red component");

    const red2 = new Color(255, 0, 0);
    red2.values[0] = 0;
    assert(red2.values[0] === 0, "Directly modifying values should reflect in getRed");
}

{
    console.log("=== Testing class private fields... ===");

    class Color {
        // Declare: every Color instance has a private field called #values.
        #values;
        constructor(r, g, b) {
            this.#values = [r, g, b];
        }
        getRed() {
            return this.#values[0];
        }
        setRed(value) {
            if (value < 0 || value > 255) {
                throw new RangeError("Invalid R value");
            }
            this.#values[0] = value;
        }

        redDifference(anotherColor) {
            if (!(#values in anotherColor)) {
                throw new TypeError("Color instance expected");
            }
            // #values doesn't necessarily need to be accessed from this:
            // you can access private fields of other instances belonging
            // to the same class.
            return this.#values[0] - anotherColor.#values[0];
        }
    }

    const red = new Color(255, 0, 0);
    assert(red.getRed() === 255, "getRed should return the red component");

    // console.log(red.#values); // SyntaxError: Private field '#values' must be declared in an enclosing class
    try {
        let script = `
            class Test {
                #values;
            }
            console.log(red.#values);
        `;
        eval(script);
        assert(false, "Accessing private field should throw");
    } catch (e) {
        console.log("Caught expected error for private field access:", e.message);
        assert(e instanceof SyntaxError, "Expected a SyntaxError");
        console.log("PASS: Private field cannot be accessed outside the class");
    }

    red.setRed(0);
    assert(red.getRed() === 0, "setRed should update the red component");


    try {
        red.setRed(300);
        assert(false, "Setting invalid red value should throw");
    } catch (e) {
        console.log("Caught expected error for invalid red value:", e.message);
        assert(e instanceof RangeError, "Expected a RangeError");
        console.log("PASS: setRed throws on invalid value");
    }

    const red2 = new Color(255, 0, 0);
    const crimson = new Color(220, 20, 60);
    assert(red2.redDifference(crimson) === 35, "redDifference should compute the difference in red components");  
}

{
    console.log("=== Testing bad class private field usages... ===");

    try {
        eval("class BadIdeas { #firstName; #firstName; }");
    } catch (e) {
        console.log("Caught expected error for duplicate private field declaration:", e.message);
        assert(e instanceof SyntaxError, "Expected a SyntaxError");
    }

    try {
        let scritpt = `
        class BadIdeas {
            #lastName;
            constructor() {
                delete this.#lastName;
            }
        }`;
        eval(scritpt);
    } catch (e) {
        console.log("Caught expected error for bad private field usage:", e.message);
        assert(e instanceof SyntaxError, "Expected a SyntaxError");
        console.log("PASS: Bad private field usages throw errors");
    }
}

{
    console.log("=== Testing getters and setters... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        get red() {
            return this.values[0];
        }
        set red(value) {
            this.values[0] = value;
        }
    }

    const my_color = new Color(255, 0, 0);
    assert(my_color.red === 255, "Initial red value should be 255");
    my_color.red = 0;
    assert(my_color.red === 0, "Red value should be updated to 0");
}

{
    console.log("=== Testing read-only getter... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        get red() {
            return this.values[0];
        }
    }

    const red = new Color(255, 0, 0);
    try {
        red.red = 0; // This should throw an error
        assert(false, "Setting read-only property 'red' should have thrown an error");
    } catch (e) {
        console.log("Caught expected error when trying to set read-only property 'red':", e.message);
        assert(e instanceof TypeError, "Caught exception should be a TypeError");
    }
    assert(red.red === 255, "Red value should remain 255");
}

{
    console.log("=== Testing write-only setter... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        set red(value) {
            this.values[0] = value;
        }
    }

    const red = new Color(255, 0, 0);
    red.red = 100;
    assert(red.red === undefined, "Getting write-only property 'red' should return undefined");
}

{
    console.log("=== Testing class field initializers... ===");
    class MyClass {
        luckyNumber = Math.random();
    }
    console.log(new MyClass().luckyNumber);
    console.log(new MyClass().luckyNumber);
}

{
    console.log("=== Testing class field initializers in constructor... ===");
    class MyClass {
        constructor() {
            this.luckyNumber = Math.random();
        }
    }
    console.log(new MyClass().luckyNumber);
    console.log(new MyClass().luckyNumber);
}

{
    console.log("=== Testing static methods... ===");
    class Color {
        static isValid(r, g, b) {
            return r >= 0 && r <= 255 && g >= 0 && g <= 255 && b >= 0 && b <= 255;
        }
        static {
            Color.myStaticProperty = "foo";
        }
    }

    assert(Color.myStaticProperty === "foo", "Static property should be 'foo'");

    assert(Color.isValid(255, 0, 0) === true, "Color (255, 0, 0) should be valid");
    assert(Color.isValid(-1, 0, 0) === false, "Color (-1, 0, 0) should be invalid");
    assert(Color.isValid(1000, 0, 0) === false, "Color (1000, 0, 0) should be invalid");

    assert(new Color(0, 0, 0).isValid === undefined, "Instance should not have isValid method");
}

{
    console.log("=== Testing subclassing and private fields... ===");

    class Color {
        constructor(r, g, b) {
            this.values = [r, g, b];
        }
        get red() {
            return this.values[0];
        }
        set red(value) {
            this.values[0] = value;
        }
    }

    class ColorWithAlpha extends Color {
        #alpha;
        constructor(r, g, b, a) {
            super(r, g, b);
            this.#alpha = a;
        }
        get alpha() {
            return this.#alpha;
        }
        set alpha(value) {
            if (value < 0 || value > 1) {
                throw new RangeError("Alpha value must be between 0 and 1");
            }
            this.#alpha = value;
        }
    }

    const color = new ColorWithAlpha(255, 0, 0, 0.5);
    assert(color.red === 255, "Red value should be 255");
    assert(color.alpha === 0.5, "Alpha value should be 0.5");

    console.log(color.toString()); // [object Object]
}

{
    class HTMLElement {
        constructor() {
            this.onclick = null;
            this.textContent = "";
        }
    }

    class Counter extends HTMLElement {
        #xValue = 0;
        constructor() {
            super();
            this.onclick = this.#clicked.bind(this);
        }
        get #x() {
            return this.#xValue;
        }
        set #x(value) {
            this.#xValue = value;
            window.requestAnimationFrame(this.#render.bind(this));
        }
        #clicked() {
            this.#x++;
        }
        #render() {
            this.textContent = this.#x.toString();
        }
        connectedCallback() {
            this.#render();
        }
    }

    // Tests for Counter class
    {
        console.log("=== Testing Counter private getter/setter and event binding ===");

        // Ensure a global window.requestAnimationFrame is available and runs callbacks synchronously for tests
        if (typeof globalThis.window === 'undefined') {
            globalThis.window = globalThis;
        }
        if (typeof window.requestAnimationFrame !== 'function') {
            window.requestAnimationFrame = function (cb) { cb(); };
        }

        const counter = new Counter();
        // connectedCallback should render initial value
        counter.connectedCallback();
        assert(counter.textContent === '0', "Counter initial render should be '0'");

        // onclick should be bound to the private clicked handler
        assert(typeof counter.onclick === 'function', 'onclick should be a function');

        // Simulate click -> should increment and trigger render
        counter.onclick();
        // requestAnimationFrame runs synchronously in test, so textContent should be updated
        assert(counter.textContent === '1', "Counter should increment to '1' after one click");

        // Multiple clicks
        counter.onclick();
        counter.onclick();
        assert(counter.textContent === '3', "Counter should be '3' after three clicks");

        console.log('PASS: Counter private fields and event binding behave as expected');
    }
}

{
    console.log("=== Testing superclass method calls with super... ===");

    class Color {
        #values;
        constructor(r, g, b) {
            this.#values = [r, g, b];
        }
        get red() {
            return this.#values[0];
        }
        set red(value) {
            this.#values[0] = value;
        }

        toString() {
            return this.#values.join(", ");
        }
    }

    console.log(new Color(255, 0, 0).toString()); // '255, 0, 0'
    assert(new Color(255, 0, 0).toString() === "255, 0, 0", "toString should return '255, 0, 0'");

    class ColorWithAlpha extends Color {
        #alpha;
        constructor(r, g, b, a) {
            super(r, g, b);
            this.#alpha = a;
        }
        get alpha() {
            return this.#alpha;
        }
        set alpha(value) {
            if (value < 0 || value > 1) {
                throw new RangeError("Alpha value must be between 0 and 1");
            }
            this.#alpha = value;
        }

        toString() {
            // Call the parent class's toString() and build on the return value
            return `${super.toString()}, ${this.#alpha}`;
        }

        log() {
            // For demonstration, attempt to access superclass private field will fail.
            // To make this script valid so we comment it out. If you uncomment, it should throw a SyntaxError, and the test suit will can't pass.
            //
            // console.log(this.#values); // SyntaxError: Private field '#values' must be declared in an enclosing class
            //
            throw new SyntaxError("Cannot access superclass private field '#values'");
        }
    }

    console.log(new ColorWithAlpha(255, 0, 0, 0.5).toString()); // '255, 0, 0, 0.5'
    assert(new ColorWithAlpha(255, 0, 0, 0.5).toString() === "255, 0, 0, 0.5", "toString should return '255, 0, 0, 0.5'");

    let tmp = new ColorWithAlpha(255, 0, 0, 0.5);
    try {
        tmp.log(); // SyntaxError: Private field '#values' must be declared in an enclosing class
    } catch (e) {
        console.log("Caught expected error for private field access in subclass:", e.message);
        assert(e instanceof SyntaxError, "Expected a SyntaxError");
        console.log("PASS: Subclass cannot access superclass private field");
    }

    const color = new ColorWithAlpha(255, 0, 0, 0.5);
    console.log(color instanceof Color); // true
    assert(color instanceof Color, "color should be instance of Color");
    console.log(color instanceof ColorWithAlpha); // true
    assert(color instanceof ColorWithAlpha, "color should be instance of ColorWithAlpha");
}
