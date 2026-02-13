// module
import value from "./json-modules-target.json" with { type: "json" };

if (value && value.ok === true) {
  console.log("OK");
}
