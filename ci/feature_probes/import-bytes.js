// module

import("./import-bytes-target.bin", { with: { type: "bytes" } }).then(function(mod) {
  var value = mod && mod.default;
  if (
    value instanceof Uint8Array &&
    value.buffer instanceof ArrayBuffer &&
    value.buffer.immutable === true &&
    value.length === 4 &&
    value[0] === 65 &&
    value[1] === 66 &&
    value[2] === 67 &&
    value[3] === 10
  ) {
    console.log("OK");
  }
}).catch(function() {});
