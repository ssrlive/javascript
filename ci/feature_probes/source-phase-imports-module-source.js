// module
// Probe: source-phase-imports-module-source
// Verify import.source(specifier) loads the target module and reads its exports.
try {
  var result = import.source("./source-phase-imports-target.js");
  if (result && typeof result.then === "function") {
    result.then(function(mod) {
      if (mod && mod.ok === true) {
        console.log("OK");
      }
    }).catch(function() {});
  }
} catch (e) {
  // syntax not supported
}
