// Feature probe for legacy-regexp
try {
  var desc = Object.getOwnPropertyDescriptor(RegExp, "$1");
  if (desc && typeof desc.get === "function") {
    console.log("OK");
  }
} catch (e) {
  // not supported
}
