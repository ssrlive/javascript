try {
  var p = eval('import("./import-attributes-target.js", { with: { type: "json" } })');
  if (p && typeof p.then === "function") {
    p.then(function(mod) {
      if (mod && mod.ok === true) {
        console.log("OK");
      }
    }).catch(function() {
      // suppress
    });
  }
} catch (e) {
  // suppress
}
