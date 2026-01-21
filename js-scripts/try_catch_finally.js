"use strict";

{
  class MyError extends Error {
    constructor(msg) {
      super(msg);
      this.name = "MyError";
    }
  }

  class OtherError extends Error {
    constructor(msg) {
      super(msg);
      this.name = "OtherError";
    }
  }

  function test(type) {
    try {
      if (type === 1) throw new MyError("这是 MyError");
      if (type === 2) throw new OtherError("这是 OtherError");
      throw new Error("这是普通 Error");
    } catch (e) {
      if (e instanceof MyError) {
        console.log("捕获到了 MyError: " + e.message);
      } else if (e instanceof OtherError) {
        console.log("捕获到了 OtherError: " + e.message);
      } else {
        console.log("捕获到了普通 Error: " + e.message);
      }
    }
  }

  test(1);
  test(2);
  test(3);
}

{
  function f() {
    try {
      console.log(0);
      throw "bogus";
    } catch (e) {
      console.log(1);
      // 这个 return 语句会被挂起直到 finally 块结束
      return true;
      console.log(2); // 不可达
    } finally {
      console.log(3);
      return false; // 覆盖前面的“return”
      console.log(4); // 不可达
    }
    // 现在执行“return false”
    console.log(5); // 不可达
  }
  console.log(f()); // 0、1、3、false
}

{
  function f() {
    try {
      throw "bogus";
    } catch (e) {
      console.log("捕获内部的“bogus”");
      throw e;
    } finally {
      return false; // 覆盖前面的“throw”
    }
    // 现在执行“return false”
  }

  try {
    console.log(f());
  } catch (e) {
    // 这永远不会抵达！
    // f() 执行时，`finally` 块返回 false，而这会覆盖上面的 `catch` 中的 `throw`
    console.log("捕获外部的“bogus”");
  }

  // 日志：
  // 捕获内部的“bogus”
  // false
}

{
  console.log("=== Test variable scoping in try-catch-finally ===");
  (function(x) {
    try {
      let x = 'inner';
      throw 0;
    } catch (e) {
      if (x !== 'outer') {
        throw new Error('Test failed: x should be "outer"');
      }
    }
  })('outer');
}

{
  console.log("=== finally block let declaration only shadows outer parameter value 2 ===");
  (function(x) {
    try {
      let x = 'middle';
      {
        let x = 'inner';
        throw 0;
      }
    } catch(e) {

    } finally {
      if (x !== 'outer') {
        throw new Error('Test failed: x should be "outer"');
      }
    }
  })('outer');
}

{
  console.log("=== verify context in finally block 1 ===");
  
  function fx() {}

  (function(x) {
    try {
      let x = 'inner';
      throw 0;
    } catch(e) {
    } finally {
      fx();
      if (x !== 'outer') {
        throw new Error('Test failed: x should be "outer"');
      }
    }
  })('outer');
}

{
  console.log("=== try block let declaration only shadows outer parameter value 2 ===");

  (function(x) {
    try {
      let x = 'middle';
      {
        let x = 'inner';
        throw 0;
      }
    } catch (e) {
      if (x !== 'outer') {
        throw new Error('Test failed: x should be "outer"');
      }
    }
  })('outer');
}

{
  console.log("=== verify context in try block 1 ===");
  
  function f3() {}

  (function(x) {
    try {
      let x = 'inner';
      throw 0;
    } catch (e) {
      f3();
      if (x !== 'outer') {
        throw new Error('Test failed: x should be "outer"');
      }
    }
  })('outer');
}

return true;
