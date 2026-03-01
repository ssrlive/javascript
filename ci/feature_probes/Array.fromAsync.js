// Feature probe: Array.fromAsync
(async () => {
  const result = await Array.fromAsync([1, 2, 3]);
  if (result.length !== 3 || result[0] !== 1 || result[1] !== 2 || result[2] !== 3) {
    throw new Error("Array.fromAsync failed");
  }
  console.log("OK");
})();
