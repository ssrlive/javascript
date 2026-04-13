// module
// Probe for `import defer * as ns from "..."` syntax and deferred evaluation.
import defer * as ns from "./import-defer-target.js";

try {
  if (ns.ok === true) {
    console.log("OK");
  } else {
    throw new Error("Unexpected result");
  }
} catch (e) {
  console.log("NO. reason: " + e);
}
