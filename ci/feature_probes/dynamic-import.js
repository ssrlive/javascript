import("./dynamic-import-target.js").then(function(mod) {
  if (mod && mod.ok === true) {
    console.log("OK");
  }
}).catch(function() {
  // suppress
  console.log("NO");
});
