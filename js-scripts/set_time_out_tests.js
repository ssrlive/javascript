function Person() {
  // The Person() constructor defines `this` as itself.
  this.age = 0;

  this.intervalId = setInterval(function growUp() {
    // In nonstrict mode, the growUp() function defines `this`
    // as the global object, which is different from the `this`
    // defined by the Person() constructor.
    console.log('[Person] growUp called. this.age:', this ? this.age : undefined);
  }, 1000);
}

function PersonFixed() {
  this.age = 0;

  // Arrow functions capture `this` lexically.
  this.intervalId = setInterval(() => {
    this.age++;
    console.log('[PersonFixed] Arrow function called. this.age:', this.age);
  }, 1000);
}

console.log('1. Testing Person (Broken `this` binding)...');
const p = new Person();

setTimeout(() => {
  console.log('   Person age after 1.1s:', p.age); // Expected: 0
  clearInterval(p.intervalId);

  console.log('\n2. Testing PersonFixed (Arrow function)...');
  const pf = new PersonFixed();

  setTimeout(() => {
    console.log('   PersonFixed age after 1.1s:', pf.age); // Expected: 1
    clearInterval(pf.intervalId);
  }, 1100);

}, 1100);
