# 机械制造工艺动态知识图谱（本地运行版）

本项目使用本地 Neo4j Community 5.26.28 保存《机械制造工艺》课程图谱，Python FastAPI 提供分层查询接口，Cytoscape.js 负责动态图形展示。

图谱不会一次加载全部节点。打开首页时只读取七个 `Chapter`，之后根据用户操作逐层查询数据库：

```text
Chapter --HAS_SECTION--> Section --CONTAINS--> Knowledge
Knowledge --HAS_CONCEPT--> Concept
Knowledge --RELATED_TO--> Knowledge
```

## 1. 数据规模

- Chapter：7
- Section：47
- Knowledge：283
- Concept：1449
- 有效关系：2389
- 原 CSV 中有 1 条关系端点不存在，导入时自动跳过

数据包保存在：

```text
data/机械制造工艺知识图谱_Neo4j导入包.zip
```

## 2. 启动方法

在项目目录打开 PowerShell：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\start.ps1
```

访问地址：

- 知识图谱：http://127.0.0.1:8000
- API 文档：http://127.0.0.1:8000/docs
- Neo4j Browser：http://127.0.0.1:7474
- Neo4j 用户名：`neo4j`
- Neo4j 密码：使用你在本机 `.env` 的 `NEO4J_PASSWORD` 中设置的强密码

停止服务：

```powershell
.\stop.ps1
```

## 3. 图谱使用方法

1. 首页只显示七个蓝色 Chapter 节点。
2. 单击 Chapter，加载该章的绿色 Section。
3. 单击 Section，加载该节的橙色 Knowledge。
4. 单击 Knowledge，左侧显示知识点名称、所属章节、所属小节和知识内容。
5. 双击 Knowledge，加载灰色 Concept 和通过 `RELATED_TO` 连接的其他知识点。
6. 可拖动节点、拖动画布、滚轮缩放。
7. 左侧搜索支持按中文节点名称和知识内容查询；最多返回 20 个结果，不会加载全图。
8. 点击“恢复示例图”可清空当前画布并重新显示七个章节。

节点颜色：

- Chapter：蓝色
- Section：绿色
- Knowledge：橙色
- Concept：灰色

## 4. 重新导入数据

`seed.ps1` 会删除 Neo4j 中的现有节点，再导入 ZIP 内的五个 CSV。旧的 WorkOrder、Machine、QualityReport 等测试节点会被彻底删除。

```powershell
.\seed.ps1
```

也可以直接运行 Python：

```powershell
cd backend
..\.venv\Scripts\python.exe scripts\import_process_graph.py
```

导入脚本会：

1. 读取 UTF-8 BOM CSV。
2. 清空原数据库节点。
3. 删除旧制造业 Demo 的冲突约束。
4. 把章节名称中的下划线转换为空格。
5. 按第一章至第七章设置显示顺序。
6. 创建四类节点和四类关系。
7. 跳过端点不存在或类型不允许的关系。

## 5. 主要文件

```text
backend/app/main.py                    FastAPI 入口及网页服务
backend/app/routes.py                  分层展开、搜索、详情接口
backend/app/context_graph_client.py    Neo4j 驱动与通用 Cypher 执行
backend/scripts/import_process_graph.py 新课程图谱导入脚本
web/index.html                         图谱页面
web/app.js                             分层加载及动态展开逻辑
web/styles.css                         页面与详情面板样式
data/机械制造工艺知识图谱_Neo4j导入包.zip 课程数据
runtime/                               便携 Neo4j 和 Java 21
start.ps1                              一键启动
stop.ps1                               停止服务
seed.ps1                               清库并重新导入课程数据
```

## 6. 分层接口

```text
GET  /api/process-graph/chapters
POST /api/process-graph/expand
GET  /api/process-graph/knowledge/{knowledge_id}
POST /api/process-graph/search
```

`/api/process-graph/expand` 请求示例：

```json
{
  "element_id": "CH_2"
}
```

这里的 `element_id` 使用 CSV 中的业务 ID，而不是 Neo4j 内部自增编号。

## 7. 换电脑后的首次安装

项目已经内置 Neo4j 和 Java。新电脑只需安装 Python 3.11 或 3.12：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\setup.ps1
.\start.ps1
.\seed.ps1
```

## 8. 常见问题

### Neo4j 未连接

查看 `logs/neo4j.err.log`。Neo4j 通常需要 5～20 秒启动。

### 页面还是旧数据

运行 `seed.ps1`，然后刷新浏览器并点击“恢复示例图”。

### K 盘映射说明

Neo4j 5.26 的 Windows 启动器不能可靠处理中文安装路径。`start.ps1` 会临时将项目映射为 `K:`；文件没有被移动，`stop.ps1` 会取消映射。

### 模型 API

图谱浏览、搜索、详情和动态展开不需要模型 API。AI 自动抽取和对话不在当前本地演示范围内。
