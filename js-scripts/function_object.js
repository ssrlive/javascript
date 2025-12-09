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
