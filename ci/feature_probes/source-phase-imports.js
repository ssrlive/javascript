try {
  var emitted = false;
  function emitOK() {
    if (!emitted) {
      emitted = true;
      console.log("OK");
    }
  }

  var result = import.source("./source-phase-imports-target.js");

  // Synchronous auxiliary check: if the call shape is accepted
  // (returns object/function), emit an immediately observable signal for the runner.
  if (result && (typeof result === "object" || typeof result === "function")) {
    emitOK();
  }

  // Keep the original asynchronous probe: validate final namespace content.
  if (result && typeof result.then === "function") {
    result
      .then(function(mod) {
        if (mod && mod.ok === true) {
          emitOK();
        }
      })
      .catch(function() {
        // suppress
      });
  }
} catch (e) {
  // suppress
}
