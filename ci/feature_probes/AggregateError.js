// Feature probe for AggregateError.
// Emit only "OK" when minimum constructor behavior is available.
try {
  if (typeof AggregateError !== "function") {
    throw new Error("AggregateError missing");
  }

  const err = new AggregateError([], "msg");
  if (!err || typeof err !== "object") {
    throw new Error("AggregateError construction failed");
  }

  console.log("OK");
} catch (_) {
  // unsupported
}
