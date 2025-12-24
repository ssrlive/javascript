function MyError(message) {
  this.message = message || "";
}

MyError.prototype.toString = function() { return "MyError: " + this.message; };

try {
  throw new MyError('dbg-test');
} catch(e) {
  console.log('CAUGHT constructor==', e.constructor===MyError);
  console.log('CAUGHT constructor.name =', e.constructor && e.constructor.name);
  console.log('CAUGHT message =', e.message);
  console.log('CAUGHT toString =', e.toString());
}

(function(){
    console.log('msg', "OK");
})()


{
  function Person(name){ this.name = name; }
  Person.prototype.greet = function(){ return `hi ${this.name}`; };

  const p = new Person('A');

  console.log(p.greet());                // 从 prototype 链上找到方法 -> "hi A"
  console.log('greet' in p);             // true（会检查原型链）

  console.log(p.hasOwnProperty('greet'));// false（不是自身属性）
  console.log(Object.prototype.hasOwnProperty.call(p, 'greet'));// false（不是自身属性）

  console.log(p.hasOwnProperty('name'));// true（是自身属性）
  console.log(Object.prototype.hasOwnProperty.call(p, 'name'));// true（是自身属性）
}
