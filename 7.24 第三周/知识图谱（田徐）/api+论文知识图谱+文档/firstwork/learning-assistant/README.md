# AI 助学引擎目录

`learning-assistant/` 是石榴软件 AI 助学功能的独立能力目录，组织方式参考 `ppt-master/`。

它和 `Pomegranate/` 保持同级，便于后续单独维护助学规则、工作流、资源模板、题库和评分策略。

## 当前 MVP

当前版本只实现最小流程：

```text
学习目标输入 -> 目标解析 -> 阶段计划生成 -> 阶段任务展示
```

完整闭环会在后续继续扩展：

```text
目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整
```

## 目录结构

```text
learning-assistant/
├─ README.md
├─ templates/
│  └─ plan_template.json
└─ skills/
   └─ learning-assistant/
      ├─ SKILL.md
      ├─ workflows/
      │  └─ generate-learning-plan.md
      └─ references/
         ├─ planning-rules.md
         └─ scoring-rules.md
```

## 文件说明

- `skills/learning-assistant/SKILL.md`：AI 助学 skill 总说明，描述目标、适用场景、输入、输出和工作流。
- `skills/learning-assistant/workflows/generate-learning-plan.md`：初版学习计划生成工作流。
- `skills/learning-assistant/references/planning-rules.md`：学习计划生成规则。
- `skills/learning-assistant/references/scoring-rules.md`：后续评分和计划调整规则。
- `templates/plan_template.json`：结构化学习计划模板。

## 和 Pomegranate 的关系

石榴软件前端页面位于：

```text
Pomegranate/src/pages/learning-assistant/index.tsx
```

Tauri 后端 command/service 位于：

```text
Pomegranate/src-tauri/src/commands/learning_assistant.rs
Pomegranate/src-tauri/src/services/learning_assistant.rs
```

页面默认使用的助学目录路径为：

```text
../learning-assistant
```

因此请保持：

```text
pomegranate-ai-ppt/
├─ Pomegranate/
└─ learning-assistant/
```

## 后续扩展方向

后续可以在本目录继续增加：

- `resources/`：课程资源、教材、视频、链接、知识点资料。
- `question-bank/`：题库、测试题、答案和解析。
- `rubrics/`：评分细则、能力等级描述。
- `records/` 或数据库接入说明：学习进度、阶段测试记录、计划调整记录。
- 更多 workflow：资源推荐、阶段测试、计划调整、学习报告生成。

真实资源和题库接入后，应让 Pomegranate 后端 service 读取这些结构，而不是把内容写死在前端页面里。
