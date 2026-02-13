try {
  // Probe syntax + runtime behavior for import.defer.
  // Newer behavior returns a Promise; older behavior may return a namespace object.
  var p = import.defer("./import-defer-target.js");
  if (p && typeof p.then === "function") {
    // Emit synchronously when call shape is supported.
    console.log("OK");
  } else if (p && typeof p === "object" && p.ok === true) {
    console.log("OK");
  }
} catch (e) {
  // unsupported
}
