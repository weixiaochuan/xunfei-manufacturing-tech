# 机械制造工艺动态知识图谱

本目录是为后续接入 firstwork/Pomegranate 准备的独立本地服务。本阶段不接入桌面 UI。

当前真实交付状态：

- 已提供 FastAPI 后端、静态 Cytoscape.js 前端、课程 ZIP 数据导入器和测试。
- 需要外部 Neo4j，或者使用本目录的 Docker Compose 启动独立 Neo4j 容器。
- 未内置便携 Neo4j、Java 运行时或一键离线运行包。

## 数据与能力

保留的课程图谱能力：

- Neo4j 课程知识图谱；
- `Chapter -> Section -> Knowledge -> Concept` 分层动态加载；
- `RELATED_TO` 知识点关联关系；
- 中文名称和正文搜索；
- 知识详情查询；
- Cytoscape.js 图谱展示；
- 章节列表、节点展开、知识详情和搜索四类核心接口；
- 课程 ZIP 数据导入器；
- 现有 7 章、47 节、283 知识点、1449 概念和有效关系数据。

保留但本阶段标记为 OPTIONAL/PARTIAL 的能力：

- 通用制造业 Agent；
- SSE 对话；
- 向量检索；
- GDS；
- memory backend。

这些能力不是下一阶段 Pomegranate 基础接入的必需项。

## 目录结构

```text
backend/                          FastAPI 服务和测试
backend/app/routes.py              安全课程图谱 API
backend/scripts/import_process_graph.py
                                   安全课程 ZIP 导入器
web/                               静态 Cytoscape.js 前端
data/*Neo4j*.zip                   课程导入包
cypher/                            可选 schema/GDS 辅助脚本
docker-compose.yml                 可选独立 Neo4j
docker-compose.prod.yml            可选 Neo4j + backend
```

## 配置

复制 `.env.example`：

```powershell
Copy-Item .env.example .env
```

关键配置：

```env
MEMORY_BACKEND=bolt
NEO4J_URI=neo4j://localhost:7687
NEO4J_USERNAME=neo4j
NEO4J_PASSWORD=change-me
NEO4J_DATABASE=mechanical_process_graph
ENABLE_ADMIN_ROUTES=false
```

`ENABLE_ADMIN_ROUTES=false` 会默认关闭通用 `/api/cypher`。接入桌面应用时必须保持关闭。

说明：

- Neo4j Enterprise 可使用独立数据库名，例如 `mechanical_process_graph`。
- Neo4j Community 通常只能使用默认 `neo4j` 用户库；这种情况下必须使用独立容器和独立 volume，不能和 firstwork 其他图谱数据库混用。

## 安装

```powershell
.\setup.ps1
```

或者手动安装：

```powershell
cd backend
python -m venv ..\.venv
..\.venv\Scripts\python.exe -m pip install -e ".[dev]"
```

## 启动 Neo4j

使用你已有的外部 Neo4j，或启动本目录独立 Docker Neo4j：

```powershell
docker compose up -d
```

本项目没有 Next.js 前端。静态前端在 `web/`，由 FastAPI 后端直接提供。

## 导入课程数据

导入器会清空目标数据库中的图谱数据，因此默认拒绝执行。必须显式指定数据库并确认：

```powershell
.\seed.ps1 -Database mechanical_process_graph -ConfirmReset
```

如果使用独立 Neo4j Community 容器的默认库：

```powershell
.\seed.ps1 -Database neo4j -ConfirmReset -AllowDefaultDatabase
```

不要把导入器指向 firstwork 或 Pomegranate 正在使用的其他图谱数据库。

## 启动后端和 web 前端

```powershell
.\start.ps1
```

访问：

- 图谱页面：`http://127.0.0.1:8000`
- API 文档：`http://127.0.0.1:8000/docs`
- Neo4j Browser：`http://127.0.0.1:7474`

停止后端：

```powershell
.\stop.ps1
```

`stop.ps1` 只停止本模块后端，不会停止外部 Neo4j 或 Docker 服务。

## 下一阶段安全接口

Pomegranate 下一阶段只应调用这些接口：

```text
GET  /api/process-graph/chapters
POST /api/process-graph/expand
GET  /api/process-graph/knowledge/{knowledge_id}
POST /api/process-graph/search
GET  /health
```

不应暴露给桌面应用：

- 任意 `/api/cypher`；
- 无限制 Cypher；
- 管理员导入；
- 危险写入接口；
- 未授权的通用 Agent 工具。

## 测试

```powershell
cd backend
python -m pytest tests/ -v
```

测试使用 mock Neo4j，不连接真实数据库，覆盖章节列表、节点展开、知识详情和中文搜索四类核心接口。
