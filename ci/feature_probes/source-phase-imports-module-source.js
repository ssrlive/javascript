// module

try {
  var emitted = false;
  function emitOK() {
    if (!emitted) {
      emitted = true;
      console.log("OK");
    }
  }

  // Synchronous auxiliary check for call-shape support.
  var result = import.source("./source-phase-imports-target.js");
  if (result && (typeof result === "object" || typeof result === "function")) {
    emitOK();
  }

  // Keep asynchronous validation: resolved value should be object-like.
  if (result && typeof result.then === "function") {
    result.then(function(modSource) {
      if (modSource && typeof modSource === "object") {
        emitOK();
      }
    }).catch(function() {
      // suppress
    });
  }
} catch (e) {
  // suppress
}
