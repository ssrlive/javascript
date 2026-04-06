(async function() {
  try {
    const promise = import("./dynamic-import-target.js");
    if (!promise || typeof promise.then !== "function") {
      throw new Error("import did not return a promise");
    }
    const mod = await promise;
    // console.log(mod && mod.ok === true ? "OK" : "NO");
    if (mod && mod.ok === true) {
      console.log("OK");
    } else {
      throw new Error("Module did not have expected export");
    }
  } catch (e) {
    console.log("NO. reason: " + e);
  }
})();
