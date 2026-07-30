# Classroom Cloud Module

这里是学生端/教师端班级联动的云端归档入口。

该模块负责：

- 班级创建、归档、删除。
- 教师、助教、学生成员关系。
- 邀请、加入、移除。
- 班级消息、作业、学习事件。
- 教师端汇总视图和学生端个人视图的数据权限。

实现规则：

- 当前可以先在 `services/account-server/src/classroom-*` 中实现，再按规模拆到这里。
- 所有接口从 Bearer session 解析当前 `platformUserId`。
- 不接受客户端传入的 `owner_user_id`、`teacher_id` 或 `student_id` 作为授权依据。
- 任意 `class_id` 操作必须先查当前账号在 `class_memberships` 中的有效角色。

详细约束见 `docs/account-classroom-isolation.md`。
