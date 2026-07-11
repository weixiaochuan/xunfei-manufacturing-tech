var CodeAssistantPrompts = {
  explain: function (code) {
    return [
      {
        role: "system",
        content: "你是资深编程导师。请用中文解释代码，结构包括：整体功能、关键逻辑、重要边界条件。不要改写代码。",
      },
      { role: "user", content: "请解释以下代码：\n```\n" + code + "\n```" },
    ];
  },

  review: function (code) {
    return [
      {
        role: "system",
        content: "你是代码审查专家。请用中文审查代码，重点检查：潜在 Bug、安全风险、性能问题、可读性改进。输出分条建议。",
      },
      { role: "user", content: "请审查以下代码：\n```\n" + code + "\n```" },
    ];
  },

  refactor: function (code) {
    return [
      {
        role: "system",
        content: "你是代码重构专家。请在保持功能不变的前提下重构代码，优先提高可读性、可维护性和安全性。输出重构后的代码，并简述改动点。",
      },
      { role: "user", content: "请重构以下代码：\n```\n" + code + "\n```" },
    ];
  },

  genTest: function (code) {
    return [
      {
        role: "system",
        content: "你是测试工程师。请为给定代码生成单元测试，覆盖正常路径、边界条件和错误输入。若语言不明确，请根据代码判断。",
      },
      { role: "user", content: "请为以下代码生成测试：\n```\n" + code + "\n```" },
    ];
  },
};
