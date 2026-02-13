// feature probe for 'regexp-lookbehind'
try {
  var re = /(?<=a)b/;
  var m = re.exec("ab");
  if (m && m[0] === "b") {
    console.log("OK");
  } else {
    console.log("UNSUPPORTED");
  }
} catch (e) {
  console.log("UNSUPPORTED");
}
