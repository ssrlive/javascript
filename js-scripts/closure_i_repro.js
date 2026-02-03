(function() {
  function f() {
    for (var i=0; i<3; i++) {
      Promise.resolve().then(function() { console.log('closure i=', typeof i, i); });
      break;
    }
  }
  f();
})();
