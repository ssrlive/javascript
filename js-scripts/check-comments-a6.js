class Test262Error extends Error {}
function decimalToHexString(n) { return ('0000' + n.toString(16).toUpperCase()).slice(-4); }
for (var indexI = 0; indexI <= 65535; indexI++) {
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
