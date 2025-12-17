function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "断言失败");
  }
}

const formatArg = (arg) => {
  if (Array.isArray(arg)) {
    // 打印一个无序列表
    return arg.map((part) => `- ${part}`).join("\n");
  }
  if (arg.toString === Object.prototype.toString) {
    // 这个对象会被序列化为“[object Object]”。
    // 我们来打印更漂亮的东西。
    return JSON.stringify(arg);
  }
  return arg;
};

const print = (segments, ...args) => {
  // 对于形式良好的模板字面量，总是有 N 个 args 和 (N+1) 个字符串 segments。
  let message = segments[0];
  segments.slice(1).forEach((segment, index) => {
    message += formatArg(args[index]) + segment;
  });
  return message;
};

const todos = ["学习 JavaScript", "学习 Web API", "构建网站", "利润！"];

const progress = { javascript: 20, html: 50, css: 10 };

let res = print `我需要做：
${todos}
当前进度为：${progress}
`;

console.log(res);

assert(
  res === `我需要做：
- 学习 JavaScript
- 学习 Web API
- 构建网站
- 利润！
当前进度为：{"css":10,"html":50,"javascript":20}
`,
  "模板字面量标签功能错误"
);

// let res = print(["我需要做：\n", "\n当前进度为：", "\n"], todos, progress);
// console.log(res);
