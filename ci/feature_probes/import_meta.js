import("./import_meta_target.js").then(function(mod) {
  if (mod && mod.ok === true) {
    console.log("OK");
  }
}).catch(function() {
  // suppress
});
