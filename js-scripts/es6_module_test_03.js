"use strict";

import { PI, E, add } from "./es6_module_export.js";
import multiply from "./es6_module_export.js";

// Test imported values
console.log("PI:", PI);
console.log("E:", E);
console.log("add(3, 4):", add(3, 4));
console.log("multiply(3, 4):", multiply(3, 4));

// Verify values
let pi_ok = Math.abs(PI - 3.14159) < 0.0001;
let e_ok = Math.abs(E - 2.71828) < 0.0001;
let add_ok = add(3, 4) === 7;
let multiply_ok = multiply(3, 4) === 12;

return pi_ok && e_ok && add_ok && multiply_ok;
