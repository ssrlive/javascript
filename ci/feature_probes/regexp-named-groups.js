// feature probe for 'regexp-named-groups'
try {
  var m = /(?<g>a)/.exec("a");
  if (m && m.groups && m.groups.g === "a") {
    console.log("OK");
  } else {
    console.log("UNSUPPORTED 1");
  }
} catch (e) {
  console.log("UNSUPPORTED 2");
}
