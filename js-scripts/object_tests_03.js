"use strict";

function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

{
  console.log("Testing object literal property names...");

  const value1 = 10;
  const value2 = 20;
  const value3 = 30;
  const obj = {
    property1: value1, // property name may be an identifier
    2: value2, // or a number
    "property n": value3, // or a string
  };
  console.log("obj:", obj);
  assert(obj.property1 === 10, "obj.property1 should be 10");
  assert(obj[2] === 20, "obj[2] should be 20");
  assert(obj["property n"] === 30, 'obj["property n"] should be 30');
}

{
  console.log("Testing optional chaining...");

  let cond = false;

  let x;
  if (cond) {
    x = { greeting: "hi there" };
  }
  console.log(x?.greeting);
}

{
  console.log("=== Testing nested object literals... ===");

  const myHonda = {
    color: "red",
    wheels: 4,
    engine: { cylinders: 4, size: 2.2 },
  };

  console.log("myHonda:", myHonda);
  assert(myHonda.color === "red", "myHonda.color should be 'red'");
  assert(myHonda.wheels === 4, "myHonda.wheels should be 4");
  assert(myHonda.engine.cylinders === 4, "myHonda.engine.cylinders should be 4");
  assert(myHonda.engine.size === 2.2, "myHonda.engine.size should be 2.2");
}

{
  console.log("=== Testing constructor functions... ===");

  function Car(make, model, year, owner) {
    this.make = make;
    this.model = model;
    this.year = year;
    this.owner = owner;
  }

  const myCar = new Car("Eagle", "Talon TSi", 1993);
  console.log("myCar:", myCar);
  assert(myCar.make === "Eagle", "myCar.make should be 'Eagle'");
  assert(myCar.model === "Talon TSi", "myCar.model should be 'Talon TSi'");
  assert(myCar.year === 1993, "myCar.year should be 1993");

  const randCar = new Car("Nissan", "300ZX", 1992);
  const kenCar = new Car("Mazda", "Miata", 1990);


  function Person(name, age, sex) {
    this.name = name;
    this.age = age;
    this.sex = sex;
  }  

  const rand = new Person("Rand McKinnon", 33, "M");
  const ken = new Person("Ken Jones", 39, "M");
  console.log("ken:", ken);

  const car1 = new Car("Eagle", "Talon TSi", 1993, rand);
  console.log("car1:", car1);
  const car2 = new Car("Nissan", "300ZX", 1992, ken);

  console.log(car2.owner.name);

  car1.color = "black";
}

{
  console.log("=== Using the Object.create() method ===");

  // Animal properties and method encapsulation
  const animalProto = {
    type: "Invertebrates", // Default value of properties
    displayType() {
      // Method which will display the type of animal
      console.log(this.type);
    },
  };

  // Create a new animal type called `animal`
  const animal = Object.create(animalProto);
  animal.displayType(); // Logs: Invertebrates

  // Create a new animal type called fish
  const fish = Object.create(animalProto);
  fish.type = "Fishes";
  fish.displayType(); // Logs: Fishes
}

{
  console.log("=== Testing dot and bracket notation for object properties... ===");
  const myCar = {
    make: "Ford",
    model: "Mustang",
    year: 1969,
  };
  // Dot notation
  myCar.make = "Ford";
  myCar.model = "Mustang";
  myCar.year = 1969;

  // Bracket notation
  myCar["make"] = "Ford";
  myCar["model"] = "Mustang";
  myCar["year"] = 1969;
}

{
  console.log("=== Testing dynamic property names in objects... ===");
  const myObj = {};
  const str = "myString";
  const rand = Math.random();
  const anotherObj = {};

  // Create additional properties on myObj
  myObj.type = "Dot syntax for a key named type";
  myObj["date created"] = "This key has a space";
  myObj[str] = "This key is in variable str";
  myObj[rand] = "A random number is the key here";
  myObj[anotherObj] = "This key is object anotherObj";
  myObj[""] = "This key is an empty string";

  console.log(myObj);
  // {
  //   type: 'Dot syntax for a key named type',
  //   'date created': 'This key has a space',
  //   myString: 'This key is in variable str',
  //   '0.6398914448618778': 'A random number is the key here',
  //   '[object Object]': 'This key is object anotherObj',
  //   '': 'This key is an empty string'
  // }
  console.log(myObj.myString); // 'This key is in variable str'
}

{
  console.log("=== Testing object key access === ");
  const myObj = {};
  const anotherObj = {};
  myObj[anotherObj] = "value";
  console.log("myObj[anotherObj]:", myObj[anotherObj]);
  assert(myObj[anotherObj] === "value", "Should be able to read back using object key");
}

{
  console.log("=== Testing difference between dot and bracket notation ===");

  const myObj = {};
  let str = "myString";
  myObj[str] = "This key is in variable str";

  console.log(myObj.str); // undefined
  assert(myObj.str === undefined, "myObj.str should be undefined");

  console.log(myObj[str]); // 'This key is in variable str'
  assert(myObj[str] === "This key is in variable str", "myObj[str] should be 'This key is in variable str'");
  console.log(myObj.myString); // 'This key is in variable str'
  assert(myObj.myString === "This key is in variable str", "myObj.myString should be 'This key is in variable str'");
}

{
  console.log("=== Testing using variables to access object properties ===");

  const myCar = {};

  // set property using variable for property name
  let propertyName = "make";
  myCar[propertyName] = "Ford";
  assert(myCar.make === "Ford", "myCar.make should be 'Ford'");

  // access different properties by changing the contents of the variable
  propertyName = "model";
  myCar[propertyName] = "Mustang";

  console.log(myCar); // { make: 'Ford', model: 'Mustang' }
  assert(JSON.stringify(myCar) === JSON.stringify({ make: 'Ford', model: 'Mustang' }), "myCar should have make 'Ford' and model 'Mustang'");

  assert(myCar.nonexistentProperty === undefined, "myCar.nonexistentProperty should be undefined");
}

{
  console.log("=== Testing Object.hasOwn function ===");

  function showProps(obj, objName) {
    let result = "";
    for (const i in obj) {
      // Object.hasOwn() is used to exclude properties from the object's
      // prototype chain and only show "own properties"
      if (Object.hasOwn(obj, i)) {
        result += `${objName}.${i} = ${obj[i]}\n`;
      }
    }
    console.log(result.trim());
  }

  const myCar = {
    make: "Ford",
    model: "Mustang",
    year: 1969,
  };
  showProps(myCar, "myCar");
}

{
  console.log("=== Testing Object.keys function ===");

  function showProps(obj, objName) {
    let result = "";
    Object.keys(obj).forEach((i) => {
      result += `${objName}.${i} = ${obj[i]}\n`;
    });
    console.log(result.trim());
  }

  const myCar = {
    make: "Ford",
    model: "Mustang",
    year: 1969,
  };
  showProps(myCar, "myCar");
}

{
  function listAllProperties(myObj) {
    let objectToInspect = myObj;
    let result = [];

    while (objectToInspect !== null) {
      result = result.concat(Object.getOwnPropertyNames(objectToInspect));
      objectToInspect = Object.getPrototypeOf(objectToInspect);
    }

    return result;
  }

  console.log("=== Testing listing all properties, including inherited ones ===");
  
  const myCar = {
    make: "Ford",
    model: "Mustang",
    year: 1969,
    subitems: {
      item1: "item1 value",
      item2: "item2 value",
    },
  };
  console.log(listAllProperties(myCar));
}

{
  console.log("=== Testing deleting object properties ===");

  // Creates a new object, myObj, with two properties, a and b.
  const myObj = { a: 5, b: 12 };

  // Removes the a property, leaving myObj with only the b property.
  delete myObj.a;
  console.log("a" in myObj); // false
  assert(!("a" in myObj), "Property 'a' should have been deleted from myObj");
}

{
  console.log("=== Testing adding properties to prototype ===");
  
  function Car(make, model, year) {
    this.make = make;
    this.model = model;
    this.year = year;
  }

  const car1 = new Car("Eagle", "Talon TSi", 1993);

  Car.prototype.color = "red";
  console.log(car1.color); // "red"
  assert(car1.color === "red", "car1.color should be 'red'");
}

{

  console.log("=== Testing different ways to define methods in objects ===");

  function functionName(params) {
    // do something
    console.log("functionName called with params:", params);
  }

  // Assigning a function to an object property
  let objectName = {};
  objectName.methodName = functionName;
  objectName.methodName("test");

  const myObj = {
    myMethod: function (params, anotherParam) {
      // do something
      console.log("myMethod called with params:", params, anotherParam);
    },

    // this works too!
    myOtherMethod(params) {
      // do something else
      console.log("myOtherMethod called with params:", params);
    },
  };
  myObj.myMethod("hello", "a");
  myObj.myOtherMethod("world");
}

{
  console.log("=== Testing adding methods to prototype ===");

  Car.prototype.displayCar = function () {
    const result = `A Beautiful ${this.year} ${this.make} ${this.model}`;
    console.log(result);
    return result;
  };
  
  function Car(make, model, year) {
    this.make = make;
    this.model = model;
    this.year = year;
  }

  const car1 = new Car("Eagle", "Talon TSi", 1993);
  car1.displayCar(); // "A Beautiful 1993 Eagle Talon TSi"
  assert(car1.displayCar() === "A Beautiful 1993 Eagle Talon TSi", "car1.displayCar() should return 'A Beautiful 1993 Eagle Talon TSi'");
}

{
  console.log("=== Testing sharing methods between objects ===");

  const manager = {
    name: "Karina",
    age: 27,
    job: "Software Engineer",
  };
  const intern = {
    name: "Tyrone",
    age: 21,
    job: "Software Engineer Intern",
  };

  function sayHi() {
    console.log(`Hello, my name is ${this.name}`);
  }

  // Add sayHi function to both objects
  manager.sayHi = sayHi;
  intern.sayHi = sayHi;
  manager.sayHi(); // Hello, my name is Karina
  intern.sayHi(); // Hello, my name is Tyrone
}

{
  console.log("=== Testing getters and setters in objects ===");

  const myObj = {
    a: 7,
    get b() {
      return this.a + 1;
    },
    set c(x) {
      this.a = x / 2;
    },
  };

  console.log(myObj.a); // 7
  assert(myObj.a === 7, "myObj.a should be 7");
  console.log(myObj.b); // 8, returned from the get b() method
  assert(myObj.b === 8, "myObj.b should be 8");
  myObj.c = 50; // Calls the set c(x) method
  console.log(myObj.a); // 25
  assert(myObj.a === 25, "myObj.a should be 25");
}

{
  console.log("=== Testing Object.defineProperties with getters and setters ===");

  const myObj = { a: 0 };

  Object.defineProperties(myObj, {
    b: {
      get() {
        return this.a + 1;
      },
    },
    c: {
      set(x) {
        this.a = x / 2;
      },
    },
  });

  myObj.c = 10; // Runs the setter, which assigns 10 / 2 (5) to the 'a' property
  console.log(myObj.b); // Runs the getter, which yields a + 1 or 6

  assert(myObj.b === 6 && myObj.a === 5, "myObj.b should be 6 and myObj.a should be 5");
}

{
  console.log("=== Testing object reference equality ===");

  // Two variables, two distinct objects with the same properties
  const fruit = { name: "apple" };
  const anotherFruit = { name: "apple" };

  assert(!(fruit == anotherFruit), "fruit == anotherFruit should be false");
  assert(!(fruit === anotherFruit), "fruit === anotherFruit should be false");
}

{
  console.log("=== Testing object reference assignment ===");

  // Two variables, a single object
  const fruit = { name: "apple" };
  const anotherFruit = fruit; // Assign fruit object reference to anotherFruit

  // Here fruit and anotherFruit are pointing to same object
  assert(fruit == anotherFruit, "fruit == anotherFruit should be true");
  assert(fruit === anotherFruit, "fruit === anotherFruit should be true");

  fruit.name = "grape";
  console.log(anotherFruit); // { name: "grape" }; not { name: "apple" }
  assert(anotherFruit.name === "grape", 'anotherFruit.name should be "grape"');
}

true;
