try {
  // Probe syntax + call-shape only; do not await module resolution.
  // Runner checks stdout synchronously, so emit immediately when supported.
  var p = eval('import("./import-attributes-target.js", { with: { type: "json" } })');
  if (p && typeof p.then === "function") {
    console.log("OK");
  }
} catch (e) {
  // unsupported
}
