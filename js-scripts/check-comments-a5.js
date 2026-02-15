"use strict";

class Test262Error extends Error {}
function toUu(n) {
  return n.toString(16).toUpperCase().padStart(4, "0");
}

var mustTest = ["000A", "000D", "2028", "2029"];
var sampleMap = Object.create(null);

for (var m = 0; m < mustTest.length; m++) {
  sampleMap[mustTest[m]] = true;
}

for (var cp = 0; cp <= 0xFFFF; cp += 0x0111) {
  sampleMap[toUu(cp)] = true;
}

var around = [0x000A, 0x000D, 0x2028, 0x2029];
for (var a = 0; a < around.length; a++) {
  for (var d = -2; d <= 2; d++) {
    var v = around[a] + d;
    if (v >= 0 && v <= 0xFFFF) {
      sampleMap[toUu(v)] = true;
    }
  }
}

var samples = Object.keys(sampleMap).sort();

for (var s = 0; s < samples.length; s++) {
  var uu = samples[s];
  try {
    var xx = String.fromCharCode("0x" + uu);
    var LineTerminators = ((uu === "000A") || (uu === "000D") || (uu === "2028") || (uu === "2029"));
    var yy = 0;
    eval("//var " + xx + "yy = -1");
    if (LineTerminators !== true) {
      if (yy !== 0) {
        console.log('FAIL at ' + uu + ' yy=' + yy);
        throw "process.exit(1)";
      }
    } else {
      if (yy !== -1) {
        console.log('FAIL at ' + uu + ' yy=' + yy + ' expected -1');
        throw "process.exit(1)";
      }
    }
  } catch (e){
    console.log('THREW at ' + uu);
    throw "process.exit(1)";
  }
}
console.log('ALL_OK');
