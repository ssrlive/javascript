try {
    console.log("Starting BigInt tests...");

    // 1. Constructor
    var b1 = BigInt(123);
    var b2 = BigInt("900719925474099100");
    console.log("b1 created: " + b1.toString()); 
    console.log("b2 created: " + b2.toString());

    if (b1.toString() !== "123") throw "b1 value mismatch";
    if (b2.toString() !== "900719925474099100") throw "b2 value mismatch";

    // 2. Methods
    console.log("Testing asIntN...");
    // 64-bit signed int of b2
    var b3 = BigInt.asIntN(64, b2);
    console.log("BigInt.asIntN(64, b2) = " + b3.toString());

    console.log("Testing asUintN...");
    var b4 = BigInt.asUintN(64, b2);
    console.log("BigInt.asUintN(64, b2) = " + b4.toString());

    console.log("Testing valueOf...");
    var v1 = b1.valueOf();
    console.log("b1.valueOf() = " + v1.toString());
    console.log("Type of b1.valueOf() = " + (typeof v1));
    console.log("Type of b1 = " + (typeof b1));

    console.log("BigInt tests finished successfully.");

} catch (e) {
    console.log("Test Failed: " + e);
    if (e.stack) console.log(e.stack);
}
