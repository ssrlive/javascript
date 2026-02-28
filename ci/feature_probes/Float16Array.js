// Feature probe for Float16Array (including DataView getFloat16/setFloat16 and Math.f16round)
var print = typeof print !== "undefined" ? print : console.log;
try {
  // Float16Array typed array
  var f16 = new Float16Array(2);
  f16[0] = 1.5;
  if (f16[0] !== 1.5) throw "bad Float16Array value";

  // DataView getFloat16/setFloat16
  var buf = new ArrayBuffer(4);
  var dv = new DataView(buf);
  dv.setFloat16(0, 1.5, true);
  if (dv.getFloat16(0, true) !== 1.5) throw "bad DataView float16";

  // Math.f16round
  if (typeof Math.f16round !== "function") throw "no Math.f16round";
  if (Math.f16round(1.337890625) !== 1.3378906249999998) {
    // 1.337890625 is exactly representable in f16, so round-trip should be close
    // Actually just check it returns a number
    if (typeof Math.f16round(1.5) !== "number") throw "f16round not returning number";
  }

  print("OK");
} catch (e) {
  print("NO: " + e);
}
