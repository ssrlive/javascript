(function() {
  "use strict";
  var handlers = [];

  function register(fn) {
    var id = handlers.length;
    handlers.push({ fn: fn, settled: false });
    return id;
  }

  function done(err) {
    for (var i = 0; i < handlers.length; i++) {
      if (!handlers[i].settled) {
        handlers[i].settled = true;
        Promise.resolve().then(function() {
          try {
            // Instrumentation: show 'i' and handler presence
            try { console.log('DBG handlers_repro: i typeof', typeof i, 'i', i, 'handlers_len', handlers.length, 'exists', !!handlers[i]); } catch (_) {}
            handlers[i].fn(err);
          } catch (e) {
            console.log('DBG handlers_repro caught ->', e && e.message ? e.message : e);
          }
        });
        break;
      }
    }
  }

  register(function() { globalThis.__handlers_called = true; });
  done();

  // Return a promise that resolves after microtasks should have run
  return Promise.resolve().then(() => Promise.resolve()).then(() => (globalThis.__handlers_called === true));
})();
