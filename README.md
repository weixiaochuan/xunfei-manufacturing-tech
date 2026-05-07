# 科大讯飞《制造工艺学》智能学习平台

## 项目简介
基于《制造工艺学》教材 + 讯飞星火API + Streamlit 开发的智能教学助手。

支持功能：
- 对话式知识问答（RAG）
- 章节知识大纲 + 思维导图
- 学情诊断 + 个性化学习路径
- 课后习题 + 知识精讲

## 仓库结构
├── data/              # 所有教材数据
│   ├── raw/           # 原始PDF、Word
│   ├── processed/     # 处理后的Markdown、JSON
│   ├── chunks/        # RAG分块
│   ├── excel/         # 习题库、知识点表
│   └── neo4j/         # 图谱导入文件
├── docs/              # 文档、模板、流程
├── scripts/           # 数据处理脚本
├── notebooks/         # Jupyter探索
└── README.md