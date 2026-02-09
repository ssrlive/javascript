// module
// Probe: top-level-await
// This file is intentionally a tiny ES module that uses a direct top-level
// `await`. The runner will execute it with `--module`; if the engine
// implements top-level await this file will print `OK`.

await Promise.resolve();
console.log('OK');

