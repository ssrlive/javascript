"use strict";

{
    console.log("Testing Set integration...");
    
    // Test constructor
    let s = new Set();
    if (s.size !== 0) throw "Empty set should have size 0";
    
    // Test add
    s.add(1);
    s.add(2);
    s.add(1); // duplicate
    
    if (s.size !== 2) throw "Set should have size 2 after adding 1, 2, 1. Got " + s.size;
    if (!s.has(1)) throw "Set should have 1";
    if (!s.has(2)) throw "Set should have 2";
    if (s.has(3)) throw "Set should not have 3";
    
    // Test constructor with iterable
    let arr = [1, 2, 3, 3];
    let s2 = new Set(arr);
    if (s2.size !== 3) throw "Set from [1, 2, 3, 3] should have size 3. Got " + s2.size;
    if (!s2.has(3)) throw "Set2 should have 3";
    
    // Test delete
    let deleted = s2.delete(2);
    if (!deleted) throw "delete(2) should return true";
    if (s2.has(2)) throw "Set2 should not have 2 after delete";
    if (s2.size !== 2) throw "Set2 should have size 2 after delete";
    
    let deletedNonExistent = s2.delete(999);
    if (deletedNonExistent) throw "delete(999) should return false";
    
    // Test clear
    s2.clear();
    if (s2.size !== 0) throw "Set2 should have size 0 after clear";
    
    // Test values()/keys()/entries() smoke test
    let s3 = new Set();
    s3.add("a");
    s3.add("b");
    
    let values = s3.values();
    let keys = s3.keys();
    let entries = s3.entries();
    
    if (values.next().value !== "a") throw "First value should be 'a'";
    if (keys.next().value !== "a") throw "First key should be 'a'";
    let firstEntry = entries.next().value;
    if (firstEntry[0] !== "a" || firstEntry[1] !== "a") throw "First entry should be ['a', 'a']";

    console.log("Set tests passed!");

    true;
}
