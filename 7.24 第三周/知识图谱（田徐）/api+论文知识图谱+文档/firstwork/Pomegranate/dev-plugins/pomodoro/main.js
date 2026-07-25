/**
 * 番茄钟插件 v1.0.0 — 示范 PluginAppAPI 任务 + 事件链路
 *
 * 功能：
 * 1. 监听 task:created → 自动开始 25 分钟番茄钟
 * 2. 25 分钟后在任务描述中追加 🍅 记录
 * 3. 注册命令 pomodoro.start 供命令面板调用
 * 4. 注册任务视图 pomodoro-today 展示今日番茄统计
 */

// ─── 模块级状态（onLoad 注册，onUnload 清理）────────
var activeTimers = {};
var offTaskCreated = null;
var offCommand = null;
var offView = null;

function startPomodoro(app, task) {
  if (activeTimers[task.id]) {
    clearTimeout(activeTimers[task.id]);
  }

  activeTimers[task.id] = setTimeout(function () {
    delete activeTimers[task.id];

    app.tasks.get(task.id).then(function (latest) {
      if (!latest) return;
      var now = new Date().toLocaleString();
      var newDesc = (latest.description || "") + "\n🍅 " + now + " — 完成 1 个番茄钟";
      return app.tasks.update(task.id, { description: newDesc });
    }).then(function () {
      try { app.notices.show("✅ 番茄钟完成：" + task.title); } catch (e) {}
    }).catch(function (e) {
      console.warn("[pomodoro] 写入失败:", e);
    });
  }, 25 * 60 * 1000);
}

function onLoad(ctx) {
  var app = ctx.app;

  // ─── 1. 监听任务创建，自动开始番茄钟 ──────
  offTaskCreated = app.events.on("task:created", function (task) {
    if (task.parentTaskId != null || task.status === "completed") return;
    try { app.notices.show("🍅 开始番茄钟：" + task.title + "（25 分钟）", 4); } catch (e) {}
    startPomodoro(app, task);
  });

  // ─── 2. 注册命令 ──────────────────────────
  offCommand = app.commands.addCommand({
    id: "pomodoro.start",
    title: "启动番茄钟",
    callback: function () {
      app.notices.show("🍅 番茄钟已启动（25 分钟）", 3);
    },
  });

  // ─── 3. 注册任务视图 ──────────────────────
  offView = app.taskViews.register({
    id: "pomodoro-today",
    label: "今日番茄",
    icon: "Timer",
    render: function (container, props) {
      var doneToday = props.tasks.filter(function (t) {
        return t.description && t.description.indexOf("🍅") !== -1;
      });
      var totalTomatoes = 0;
      doneToday.forEach(function (t) {
        var matches = (t.description || "").match(/🍅/g);
        totalTomatoes += matches ? matches.length : 0;
      });
      container.innerHTML =
        '<div style="padding:16px;display:flex;flex-direction:column;align-items:center;gap:12px">' +
        '<div style="font-size:48px">🍅</div>' +
        '<div style="font-size:20px;font-weight:600">' + totalTomatoes + ' 个番茄</div>' +
        '<div style="color:#888;font-size:13px">已完成 ' + doneToday.length + ' 项任务</div>' +
        '</div>';
    },
  });
}

function onUnload(ctx) {
  if (offTaskCreated) { offTaskCreated(); offTaskCreated = null; }
  if (offCommand) { offCommand(); offCommand = null; }
  if (offView) { offView(); offView = null; }
  Object.keys(activeTimers).forEach(function (id) {
    clearTimeout(activeTimers[id]);
  });
  activeTimers = {};
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = { onLoad: onLoad, onUnload: onUnload };
}
