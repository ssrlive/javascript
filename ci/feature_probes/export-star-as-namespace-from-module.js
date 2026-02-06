import("./export-star-as-namespace-from-module-target.js").then(function(mod) {
  if (mod && mod.exportns && mod.exportns.leaf === 1) {
    console.log("OK");
  }
}).catch(function() {
  // suppress
});
