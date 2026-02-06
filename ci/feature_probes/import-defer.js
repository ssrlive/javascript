try {
  import.defer("./import-defer-target.js").then(function(mod) {
    if (mod && mod.ok === true) {
      console.log("OK");
    }
  }).catch(function() {
    // suppress
  });
} catch (e) {
  // suppress
}
