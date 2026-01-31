"use strict";

class Test262Error extends Error {}
var hex = ["0","1","2","3","4","5","6","7","8","9","A","B","C","D","E","F"];
for (var i1 = 0; i1 < 16; i1++) {
  for (var i2 = 0; i2 < 16; i2++) {
    for (var i3 = 0; i3 < 16; i3++) {
      for (var i4 = 0; i4 < 16; i4++) {
        try {
          var uu = hex[i1] + hex[i2] + hex[i3] + hex[i4];
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
    }
  }
}
console.log('ALL_OK');
