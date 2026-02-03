console.log('START');
Promise.resolve().then(() => { console.log('MICROTASK'); });
console.log('END');
