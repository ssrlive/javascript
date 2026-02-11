var ownKeysResult = ["a", "b", Symbol("s")];
var calls = [];
var proxy = new Proxy({}, {
  ownKeys: function() {
    calls.push('ownKeys');
    return ownKeysResult;
  },
  getOwnPropertyDescriptor: function(_target, key) {
    calls.push('gopd:' + String(key));
    return {value: 1, writable: true, enumerable: true, configurable: true};
  },
  get: function(_target, key) {
    calls.push('get:' + String(key));
    return 1;
  }
});

var k = Object.keys(proxy);
console.log(JSON.stringify(k));
console.log(JSON.stringify(calls));
