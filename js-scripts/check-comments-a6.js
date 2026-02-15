class Test262Error extends Error {}
function decimalToHexString(n) { return ('0000' + n.toString(16).toUpperCase()).slice(-4); }

var mustTest = [0x000A, 0x000D, 0x2028, 0x2029];
var sampleMap = Object.create(null);

for (var m = 0; m < mustTest.length; m++) {
  sampleMap[mustTest[m]] = true;
}

for (var cp = 0; cp <= 0xFFFF; cp += 0x0111) {
  sampleMap[cp] = true;
}

var around = [0x000A, 0x000D, 0x2028, 0x2029];
for (var a = 0; a < around.length; a++) {
  for (var d = -2; d <= 2; d++) {
    var v = around[a] + d;
    if (v >= 0 && v <= 0xFFFF) {
      sampleMap[v] = true;
    }
  }
}

var samples = Object.keys(sampleMap).map(function(k) { return Number(k); }).sort(function(a, b) { return a - b; });

for (var s = 0; s < samples.length; s++) {
  var indexI = samples[s];
  try {
    var xx = 0;
    eval("/*var " + String.fromCharCode(indexI) + "xx = 1*/");
    var differs = xx !== 0;
  } catch (e){
    console.log('THREW at ' + decimalToHexString(indexI)); process.exit(1);
  }
  if (differs) {
    console.log('DIFFERS at ' + decimalToHexString(indexI)); process.exit(1);
  }
}
console.log('ALL_OK');
