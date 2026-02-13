(function() {
  "use strict";
  var handlers = [];

  function register(fn) {
    var id = handlers.length;
    handlers.push({ fn: fn, settled: false });
    return id;
  }

  function done(err) {
    return new Promise(function(resolve) {
      for (var i = 0; i < handlers.length; i++) {
        if (!handlers[i].settled) {
          var entry = handlers[i];
          entry.settled = true;
          Promise.resolve().then(function() {
            try {
              entry.fn(err);
            } catch (e) {
              // keep silent; result flag is asserted by test
            } finally {
              resolve();
            }
          });
          return;
        }
      }
      resolve();
    });
  }

  register(function() { globalThis.__handlers_called = true; });
  return done().then(function() {
    return globalThis.__handlers_called === true;
  });
})();
