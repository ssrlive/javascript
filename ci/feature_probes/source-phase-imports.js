try {
  import.source("./source-phase-imports-target.js").then(function(mod) {
    if (mod && mod.ok === true) {
      console.log("OK");
    }
  }).catch(function() {
    // suppress
  });
} catch (e) {
  // suppress
}
