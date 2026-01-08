"use strict";

import * as os from "os";
import {log} from "console";

// Call some OS functions
let cwd = os.getcwd();
let pid = os.getpid();
let ppid = os.getppid();

// Call some path functions
let basename = os.path.basename("/home/user/test.js");
let dirname = os.path.dirname("/home/user/test.js");
let joined = os.path.join("home", "user", "test.js");

// Log the results
log("Current working directory:", cwd);
log("Process ID:", pid);
log("Parent Process ID:", ppid);
console.log("Basename of /home/user/test.js:", basename);
console.log("Dirname of /home/user/test.js:", dirname);
console.log("Joined path:", joined);

// Return a success value
42
