# 账号隔离、班级联动与插件安全约束

本文档固定后续账号体系、学生端/教师端班级联动、题库、插件和打包时必须遵守的安全边界。

## 不可破坏的账号隔离规则

1. 云端内部身份统一使用 `platform_users.id`，外部展示可用 `account_number`，不得用用户名、昵称、邮箱或 Casdoor subject 当数据归属主键。
2. 所有云端写入表必须具备明确归属：个人数据使用 `owner_user_id`；班级共享数据使用 `class_id` 并通过成员表校验访问权限。
3. API 不接受客户端传入的 `owner_user_id` 作为授权依据；只能从 Bearer session 解析当前用户。
4. 题库、学习项目、文档库、用户文件、班级消息的查询条件必须带当前用户或当前用户所属班级。
5. 学生端不能通过改请求参数查看其他学生答题、错题、学习记录或教师视图。
6. 教师端只能查看自己创建或被授权管理的班级和成员数据。
7. 删除采用软删除时，恢复、列表、下载、复制也必须带同一账号或同一班级授权条件。
8. 本地缓存若保存云端数据，目录或数据库记录必须带 `platformUserId` 命名空间。
9. 插件读写账号数据必须通过代理接口，代理接口负责从当前账号上下文过滤数据。

当前 `services/account-server` 已具备的基础能力包括：`platform_users`、`user_sessions`、`user_files`、`documents`、`learning_projects`、`learning_project_documents`，并且主要查询已经按 `owner_user_id` 收敛。后续新增班级和题库时必须沿用这个模型。

## 班级模型建议

班级能力属于云端，不属于单机本地数据。推荐先落在 `services/account-server` 的模块和 migration 中，功能稳定后再拆 `services/classroom`。

核心表：

```sql
classrooms(
  id uuid primary key,
  owner_teacher_user_id uuid not null references platform_users(id),
  name text not null,
  course_name text,
  term_name text,
  created_at timestamptz not null default current_timestamp,
  updated_at timestamptz not null default current_timestamp,
  deleted_at timestamptz
);

class_memberships(
  class_id uuid not null references classrooms(id),
  user_id uuid not null references platform_users(id),
  role text not null check (role in ('teacher', 'assistant', 'student')),
  status text not null check (status in ('active', 'invited', 'removed')),
  joined_at timestamptz,
  primary key (class_id, user_id)
);

class_messages(
  id uuid primary key,
  class_id uuid not null references classrooms(id),
  sender_user_id uuid not null references platform_users(id),
  target_role text check (target_role in ('teacher', 'assistant', 'student')),
  body jsonb not null,
  created_at timestamptz not null default current_timestamp,
  deleted_at timestamptz
);

class_assignments(
  id uuid primary key,
  class_id uuid not null references classrooms(id),
  creator_user_id uuid not null references platform_users(id),
  title text not null,
  source_kind text not null,
  source_ref jsonb not null,
  due_at timestamptz,
  created_at timestamptz not null default current_timestamp,
  deleted_at timestamptz
);

class_learning_events(
  id uuid primary key,
  class_id uuid not null references classrooms(id),
  student_user_id uuid not null references platform_users(id),
  event_kind text not null,
  payload jsonb not null,
  created_at timestamptz not null default current_timestamp
);
```

关键约束：

- `classrooms.owner_teacher_user_id` 创建者自动成为 `teacher` 成员。
- 任何 `class_id` 接口先查 `class_memberships`，确认当前用户 `status='active'`。
- 教师创建作业、发班级消息、查看班级汇总时要求角色为 `teacher` 或 `assistant`。
- 学生提交、查看个人学习事件时要求角色为 `student`，且只能访问自己的 `student_user_id`。
- 面向班级的资料或作业如果引用文档库，必须确认引用文档属于教师账号或班级共享空间。

## 师生端信息流

推荐 API 方向：

- `POST /classes`：教师创建班级。
- `POST /classes/:classId/invites`：教师生成邀请或添加学生。
- `GET /classes`：当前账号可见班级列表。
- `POST /classes/:classId/messages`：教师/学生发消息，服务端按成员关系过滤。
- `POST /classes/:classId/assignments`：教师发布作业、练习或资料。
- `GET /classes/:classId/feed`：学生端和教师端各自收到可见消息、作业、学习事件。
- `POST /classes/:classId/learning-events`：学生端上报学习进度、题库练习、资料阅读等事件。

信息传递必须走云端，桌面本地只做缓存、离线草稿和冲突提示。离线产生的数据恢复联网后按当前账号和班级 membership 上传。

## 题库接入规则

汇总3题库接口里有 `student_id` 参数。迁入主项目后不能让前端或插件直接控制这个值。

正确方式：

1. 学生端请求当前账号的题库接口。
2. `services/account-server` 用 session 得到 `platformUserId`。
3. 云端代理或题库服务把 `platformUserId` 映射为内部 `student_id`。
4. 题库服务按 `student_id` 写入答题、错题、推荐。
5. 教师端查看班级题库统计时，通过 `class_memberships` 找到学生集合，再聚合数据。

学生练习接口还必须保留汇总3里的安全要求：

- 学生获取题目时不得返回答案或解析中的标准答案。
- 学生题目范围限定为 `review_status='已通过'` 且 `usage_scope='学生练习'`。
- 答题提交后返回反馈可以包含本次题目的必要解释，但不能泄露整库答案。
- 未登录或未加入班级的学生不能提交到班级维度。

## 插件可扩展约束

插件分本地运行时和云端市场两部分：

- 本地运行时：安装目录、启停、权限、令牌、代理 API。
- 云端市场：插件包、版本、审核、发布、适用账号/组织/班级策略。

插件安全底线：

- 插件 manifest 声明权限，默认不授权。
- 插件不能拿到真实 session token、Casdoor token 或 AI provider key。
- 插件设置以 `plugin_id` 分区，不共享 key 空间。
- 插件调用 AI、笔记、任务、文档、题库、班级接口都必须经代理层二次授权。
- 插件产生的云端数据必须写入当前账号或被授权班级，不能由插件自行传 `owner_user_id`。

## 打包检查项

每次准备打包前检查：

- `src-tauri/tauri.conf.json` 的 `bundle.resources` 包含课程图谱和学习助手资源。
- 安装包不包含 `汇总3/`、`node_modules/`、`src-tauri/target/`、运行时数据和 `.env`。
- 云端服务 `.env`、PostgreSQL 数据目录、用户文件根目录不在源码目录内。
- 本地数据目录支持迁移到非 C 盘路径。
- 登录状态、用户缓存、插件设置不跨账号复用。

这些规则是后续功能迁移的验收线。只要某个迁移项会破坏这里的隔离、联动或打包边界，就先补架构再合并功能。
