import("./import_meta_target.js").then(function(mod) {
  if (mod && mod.ok === true) {
    console.log("OK");
  } else {
    throw new Error("Unexpected module export");
  }
}).catch(function() {
  console.error("NO. reason:", arguments);
});
