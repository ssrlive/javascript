// module

import("./import-text-target.txt", { with: { type: "text" } }).then(function(mod) {
  if (mod && mod.default === "hello from text\n") {
    console.log("OK");
  }
}).catch(function() {});
