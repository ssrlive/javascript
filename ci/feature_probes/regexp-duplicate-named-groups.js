// feature probe for 'regexp-duplicate-named-groups'
try {
  var re = /(?<x>a)|(?<x>b)/;
  var m1 = re.exec("a");
  var m2 = re.exec("b");
  if (m1 && m1.groups && m1.groups.x === "a" && m2 && m2.groups && m2.groups.x === "b") {
    console.log("OK");
  } else {
    console.log("UNSUPPORTED 1");
  }
} catch (e) {
  console.log("UNSUPPORTED 2");
}
