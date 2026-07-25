use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime};
use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CreateTaskCategoryInput, CreateTaskInput, PluginCreateTaskInput, PluginTaskFilter,
    PluginTaskView, PluginUpdateTaskInput, Task, TaskCategory, TaskLinkInput, TaskQuery,
    TaskSearchHit, TaskStats, UpdateTaskCategoryInput, UpdateTaskInput,
};

pub struct TaskService;

impl TaskService {
    pub fn list(db: &Database, query: TaskQuery) -> Result<Vec<Task>, AppError> {
        db.list_tasks(query)
    }

    pub fn get(db: &Database, id: i64) -> Result<Option<Task>, AppError> {
        db.get_task(id)
    }

    /// 列出某主任务的子任务
    pub fn list_subtasks(db: &Database, parent_id: i64) -> Result<Vec<Task>, AppError> {
        db.list_subtasks(parent_id)
    }

    pub fn create(
        app: &AppHandle,
        db: &Database,
        input: CreateTaskInput,
    ) -> Result<Task, AppError> {
        let title = input.title.trim();
        if title.is_empty() {
            return Err(AppError::InvalidInput("任务标题不能为空".into()));
        }
        if let Some(p) = input.priority {
            if !(0..=2).contains(&p) {
                return Err(AppError::InvalidInput(format!("非法的 priority: {}", p)));
            }
        }
        validate_repeat(
            input.repeat_kind.as_deref(),
            input.repeat_interval,
            input.repeat_weekdays.as_deref(),
            input.repeat_count,
        )?;
        let id = db.create_task(input)?;
        let task = db
            .get_task(id)?
            .ok_or_else(|| AppError::Custom(format!("任务 {} 创建后无法读取", id)))?;
        let _ = app.emit("plugin:task:created", &task);
        Ok(task)
    }

    pub fn update(
        app: &AppHandle,
        db: &Database,
        id: i64,
        input: UpdateTaskInput,
    ) -> Result<bool, AppError> {
        if let Some(t) = input.title.as_ref() {
            if t.trim().is_empty() {
                return Err(AppError::InvalidInput("任务标题不能为空".into()));
            }
        }
        if let Some(p) = input.priority {
            if !(0..=2).contains(&p) {
                return Err(AppError::InvalidInput(format!("非法的 priority: {}", p)));
            }
        }
        validate_repeat(
            input.repeat_kind.as_deref(),
            input.repeat_interval,
            input.repeat_weekdays.as_deref(),
            input.repeat_count,
        )?;
        let ok = db.update_task(id, input)?;
        if ok {
            if let Some(task) = db.get_task(id)? {
                let _ = app.emit("plugin:task:updated", &task);
            }
        }
        Ok(ok)
    }

    pub fn toggle_status(app: &AppHandle, db: &Database, id: i64) -> Result<i32, AppError> {
        let new_status = db.toggle_task_status(id)?;
        if let Some(task) = db.get_task(id)? {
            if new_status == 1 {
                let _ = app.emit("plugin:task:completed", &task);
            } else {
                let _ = app.emit("plugin:task:updated", &task);
            }
        }
        Ok(new_status)
    }

    pub fn delete(app: &AppHandle, db: &Database, id: i64) -> Result<bool, AppError> {
        let task = db.get_task(id)?;
        let ok = db.delete_task(id)?;
        if ok {
            if let Some(task) = task {
                let _ = app.emit("plugin:task:deleted", &task);
            }
        }
        Ok(ok)
    }

    pub fn add_link(db: &Database, task_id: i64, input: TaskLinkInput) -> Result<i64, AppError> {
        db.add_task_link(task_id, input)
    }

    pub fn remove_link(db: &Database, link_id: i64) -> Result<bool, AppError> {
        db.remove_task_link(link_id)
    }

    pub fn stats(db: &Database) -> Result<TaskStats, AppError> {
        db.get_task_stats()
    }

    /// 顶栏全局搜索：keyword 空时返回空数组；limit 默认 20，封顶 50
    pub fn search(
        db: &Database,
        keyword: &str,
        limit: Option<usize>,
    ) -> Result<Vec<TaskSearchHit>, AppError> {
        let n = limit.unwrap_or(20).min(50);
        db.search_tasks(keyword, n)
    }

    /// 稍后提醒：把截止时间向后推 N 分钟 + 清提醒已触发标记
    pub fn snooze(db: &Database, id: i64, minutes: i32) -> Result<bool, AppError> {
        db.snooze_task(id, minutes)
    }

    // ─── 分类 CRUD ────────────────────────────────

    pub fn list_categories(db: &Database) -> Result<Vec<TaskCategory>, AppError> {
        db.list_task_categories()
    }

    pub fn create_category(
        db: &Database,
        mut input: CreateTaskCategoryInput,
    ) -> Result<i64, AppError> {
        input.name = input.name.trim().to_string();
        if input.name.is_empty() {
            return Err(AppError::InvalidInput("分类名称不能为空".into()));
        }
        if input.name.chars().count() > 30 {
            return Err(AppError::InvalidInput("分类名称不能超过 30 字".into()));
        }
        db.create_task_category(input)
    }

    pub fn update_category(
        db: &Database,
        id: i64,
        mut input: UpdateTaskCategoryInput,
    ) -> Result<bool, AppError> {
        if let Some(n) = input.name.as_mut() {
            *n = n.trim().to_string();
            if n.is_empty() {
                return Err(AppError::InvalidInput("分类名称不能为空".into()));
            }
            if n.chars().count() > 30 {
                return Err(AppError::InvalidInput("分类名称不能超过 30 字".into()));
            }
        }
        db.update_task_category(id, input)
    }

    pub fn delete_category(db: &Database, id: i64) -> Result<bool, AppError> {
        db.delete_task_category(id)
    }

    /// 完成本次（循环任务）：推进 due 到下一次；若循环已到终止条件则自动结束整条。
    /// 非循环任务走普通完成（切换 status）。
    pub fn complete_occurrence(
        app: &AppHandle,
        db: &Database,
        id: i64,
        all_day_base_time: &str,
    ) -> Result<(), AppError> {
        let task = db
            .get_task(id)?
            .ok_or_else(|| AppError::Custom(format!("任务 {} 不存在", id)))?;
        if task.repeat_kind == "none" {
            db.toggle_task_status(id)?;
            if let Some(updated) = db.get_task(id)? {
                let _ = app.emit("plugin:task:completed", &updated);
            }
            return Ok(());
        }
        let now = chrono::Local::now().naive_local();
        let result = advance_recurrence(&task, all_day_base_time, now);
        db.advance_task_recurrence(id, result.next_due, result.new_done_count)?;
        if let Some(updated) = db.get_task(id)? {
            if updated.status == 1 {
                let _ = app.emit("plugin:task:completed", &updated);
            } else {
                let _ = app.emit("plugin:task:updated", &updated);
            }
        }
        Ok(())
    }

    // ─── 插件专用方法（阶段 2）────────────────────────

    /// 插件列表：PluginTaskFilter → TaskQuery → Vec<PluginTaskView>
    pub fn list_for_plugin(
        db: &Database,
        filter: PluginTaskFilter,
    ) -> Result<Vec<PluginTaskView>, AppError> {
        let limit = filter.limit.unwrap_or(100).min(500);
        let offset = filter.offset.unwrap_or(0);
        let query = into_task_query(&filter);
        // 如果指定了 parent_task_id，则列子任务；否则列主任务
        let src_tasks: Vec<Task> = if let Some(pid) = filter.parent_task_id {
            db.list_subtasks(pid)?
        } else {
            db.list_tasks(query)?
        };
        let mut tasks: Vec<Task> = src_tasks
            .into_iter()
            .filter(|t| {
                // due_before
                if let Some(ref before) = filter.due_before {
                    if let Some(ref due) = t.due_date {
                        if due.as_str() > before.as_str() {
                            return false;
                        }
                    }
                }
                // due_after
                if let Some(ref after) = filter.due_after {
                    if let Some(ref due) = t.due_date {
                        if due.as_str() < after.as_str() {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            })
            .collect();
        // 分页
        let total = tasks.len();
        let start = (offset as usize).min(total);
        let end = ((offset + limit) as usize).min(total);
        tasks = tasks[start..end].to_vec();
        Ok(tasks.into_iter().map(PluginTaskView::from).collect())
    }

    /// 插件读取单任务
    pub fn get_for_plugin(db: &Database, id: i64) -> Result<Option<PluginTaskView>, AppError> {
        db.get_task(id).map(|opt| opt.map(PluginTaskView::from))
    }

    /// 插件创建任务（写 + emit）
    pub fn create_from_plugin(
        app: &AppHandle,
        db: &Database,
        input: PluginCreateTaskInput,
    ) -> Result<PluginTaskView, AppError> {
        let task_input = CreateTaskInput {
            title: input.title,
            description: input.description,
            priority: input.priority,
            important: input.important,
            due_date: input.due_at,
            remind_before_minutes: input.remind_before_minutes,
            links: None,
            repeat_kind: None,
            repeat_interval: None,
            repeat_weekdays: None,
            repeat_until: None,
            repeat_count: None,
            source_batch_id: None,
            category_id: input.category_id,
            parent_task_id: input.parent_task_id,
        };
        Self::create(app, db, task_input).map(PluginTaskView::from)
    }

    /// 插件更新任务（写 + emit）
    pub fn update_from_plugin(
        app: &AppHandle,
        db: &Database,
        id: i64,
        input: PluginUpdateTaskInput,
    ) -> Result<(), AppError> {
        let task_input = UpdateTaskInput {
            title: input.title,
            description: input.description,
            priority: input.priority,
            important: input.important,
            due_date: input.due_at,
            clear_due_date: input.clear_due_at,
            remind_before_minutes: input.remind_before_minutes,
            clear_remind_before_minutes: input.clear_remind_before_minutes,
            repeat_kind: None,
            repeat_interval: None,
            repeat_weekdays: None,
            clear_repeat_weekdays: None,
            repeat_until: None,
            clear_repeat_until: None,
            repeat_count: None,
            clear_repeat_count: None,
            category_id: input.category_id,
            clear_category_id: input.clear_category_id,
        };
        Self::update(app, db, id, task_input)?;
        Ok(())
    }

    /// 插件完成任务（写 + emit）
    pub fn complete_from_plugin(app: &AppHandle, db: &Database, id: i64) -> Result<(), AppError> {
        Self::toggle_status(app, db, id)?;
        Ok(())
    }

    /// 插件删除任务（写 + emit）
    pub fn delete_from_plugin(app: &AppHandle, db: &Database, id: i64) -> Result<(), AppError> {
        Self::delete(app, db, id)?;
        Ok(())
    }
}

// ─── 插件过滤 → TaskQuery 转换 ──────────────────

fn into_task_query(f: &PluginTaskFilter) -> TaskQuery {
    TaskQuery {
        status: f.status.as_ref().and_then(|s| match s.as_str() {
            "completed" => Some(1),
            "pending" => Some(0),
            _ => None,
        }),
        keyword: None,
        priority: None,
        category_id: f.category_id,
        uncategorized: None,
    }
}

// ─── 循环规则校验 ─────────────────────────────────

fn validate_repeat(
    kind: Option<&str>,
    interval: Option<i32>,
    weekdays: Option<&str>,
    count: Option<i32>,
) -> Result<(), AppError> {
    if let Some(k) = kind {
        if !["none", "daily", "weekly", "monthly"].contains(&k) {
            return Err(AppError::InvalidInput(format!("非法的 repeat_kind: {}", k)));
        }
    }
    if let Some(iv) = interval {
        if iv < 1 {
            return Err(AppError::InvalidInput("repeat_interval 必须 >= 1".into()));
        }
    }
    if let Some(w) = weekdays {
        if !w.trim().is_empty() {
            for part in w.split(',') {
                let n: i32 = part
                    .trim()
                    .parse()
                    .map_err(|_| AppError::InvalidInput(format!("非法的星期值: {}", part)))?;
                if !(1..=7).contains(&n) {
                    return Err(AppError::InvalidInput(format!(
                        "星期值需在 1..=7 范围内: {}",
                        n
                    )));
                }
            }
        }
    }
    if let Some(c) = count {
        if c < 1 {
            return Err(AppError::InvalidInput("repeat_count 必须 >= 1".into()));
        }
    }
    Ok(())
}

// ─── 推进逻辑 ─────────────────────────────────────

pub struct AdvanceResult {
    /// None = 循环结束（task 将被标记完成）
    pub next_due: Option<String>,
    /// 推进后的 repeat_done_count（包含本次触发）
    pub new_done_count: i32,
}

/// 推进循环任务到下一次 > now 的触发时刻。
///
/// - 本次命中算一次 done
/// - 若命中后漏掉多次（如电脑关机跨越了多次周期），一次性合并跳到最新那次，但只通知一次
/// - 遇到 repeat_count / repeat_until 上限则返回 next_due=None（由调用方写 status=1）
pub fn advance_recurrence(task: &Task, all_day_base: &str, now_dt: NaiveDateTime) -> AdvanceResult {
    let mut done = task.repeat_done_count.saturating_add(1);

    // 先判断本次命中后是否已达上限
    if let Some(max) = task.repeat_count {
        if done >= max {
            return AdvanceResult {
                next_due: None,
                new_done_count: done,
            };
        }
    }

    let Some(due_raw) = task.due_date.as_ref() else {
        return AdvanceResult {
            next_due: None,
            new_done_count: done,
        };
    };
    let Some((mut date, time_part)) = split_due(due_raw) else {
        return AdvanceResult {
            next_due: None,
            new_done_count: done,
        };
    };

    let until = task
        .repeat_until
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    loop {
        let Some(next) = next_due_date(
            date,
            &task.repeat_kind,
            task.repeat_interval,
            task.repeat_weekdays.as_deref(),
        ) else {
            return AdvanceResult {
                next_due: None,
                new_done_count: done,
            };
        };

        // 已过截止日期则终止
        if let Some(u) = until {
            if next > u {
                return AdvanceResult {
                    next_due: None,
                    new_done_count: done,
                };
            }
        }

        // 计算下一次的"提醒触发时刻"，判断是否仍处于过去（漏掉）
        let remind_trigger = compute_remind_trigger(
            next,
            time_part.as_deref(),
            all_day_base,
            task.remind_before_minutes.unwrap_or(0),
        );

        let in_future = remind_trigger.map(|t| t > now_dt).unwrap_or(true);
        if in_future {
            return AdvanceResult {
                next_due: Some(compose_due(next, time_part.as_deref())),
                new_done_count: done,
            };
        }

        // 这一轮也漏了：累计一次但不通知，继续向后推
        done = done.saturating_add(1);
        if let Some(max) = task.repeat_count {
            if done >= max {
                return AdvanceResult {
                    next_due: None,
                    new_done_count: done,
                };
            }
        }
        date = next;
    }
}

/// 计算下一次 due 日期（不检查终止条件）
fn next_due_date(
    current: NaiveDate,
    kind: &str,
    interval: i32,
    weekdays: Option<&str>,
) -> Option<NaiveDate> {
    let iv = interval.max(1) as i64;
    match kind {
        "daily" => current.checked_add_signed(Duration::days(iv)),
        "weekly" => {
            let wds = weekdays.map(parse_weekdays).unwrap_or_default();
            if wds.is_empty() {
                current.checked_add_signed(Duration::days(7 * iv))
            } else {
                // 逐日向后查找下一个匹配的星期（最多 14 天兜底）
                let mut d = current;
                for _ in 0..14 {
                    d = d.checked_add_signed(Duration::days(1))?;
                    let iso = d.weekday().number_from_monday();
                    if wds.contains(&iso) {
                        return Some(d);
                    }
                }
                None
            }
        }
        "monthly" => current.checked_add_months(Months::new(iv as u32)),
        _ => None,
    }
}

fn parse_weekdays(spec: &str) -> Vec<u32> {
    spec.split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .filter(|n| (1..=7).contains(n))
        .collect()
}

/// 拆分 due_date：返回 (日期, 时间后缀)；时间后缀形如 " HH:MM:SS"（含前导空格），
/// 全天任务则为 None
fn split_due(due: &str) -> Option<(NaiveDate, Option<String>)> {
    let head = due.get(..10)?;
    let date = NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()?;
    if due.len() > 10 {
        Some((date, Some(due[10..].to_string())))
    } else {
        Some((date, None))
    }
}

fn compose_due(date: NaiveDate, time_part: Option<&str>) -> String {
    match time_part {
        Some(t) => format!("{}{}", date.format("%Y-%m-%d"), t),
        None => date.format("%Y-%m-%d").to_string(),
    }
}

/// 计算某一次 due 对应的"提醒触发时刻" = due_datetime - remind_before_minutes
fn compute_remind_trigger(
    date: NaiveDate,
    time_part: Option<&str>,
    all_day_base: &str,
    remind_before_minutes: i32,
) -> Option<NaiveDateTime> {
    let dt_str = match time_part {
        Some(t) => format!("{}{}", date.format("%Y-%m-%d"), t),
        None => format!("{} {}", date.format("%Y-%m-%d"), all_day_base),
    };
    // 兼容 'HH:MM' 和 'HH:MM:SS'
    let parsed = NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M"))
        .ok()?;
    parsed.checked_sub_signed(Duration::minutes(remind_before_minutes as i64))
}

// ─── 单元测试 ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: 1,
            title: "测试任务".into(),
            description: Some("描述".into()),
            priority: 0,
            important: true,
            status: 0,
            due_date: Some("2026-06-07 10:00:00".into()),
            completed_at: None,
            created_at: "2026-06-01".into(),
            updated_at: "2026-06-01".into(),
            remind_before_minutes: Some(15),
            reminded_at: Some("2026-06-07 09:45:00".into()),
            repeat_kind: "none".into(),
            repeat_interval: 1,
            repeat_weekdays: None,
            repeat_until: None,
            repeat_count: None,
            repeat_done_count: 0,
            source_batch_id: Some("batch-abc-123".into()),
            category_id: Some(5),
            parent_task_id: None,
            subtask_done: 0,
            subtask_total: 0,
            links: vec![],
        }
    }

    // ─── into_task_query ──────────────────────

    #[test]
    fn test_into_task_query_status_pending() {
        let f = PluginTaskFilter {
            status: Some("pending".into()),
            ..Default::default()
        };
        let q = into_task_query(&f);
        assert_eq!(q.status, Some(0));
    }

    #[test]
    fn test_into_task_query_status_completed() {
        let f = PluginTaskFilter {
            status: Some("completed".into()),
            ..Default::default()
        };
        let q = into_task_query(&f);
        assert_eq!(q.status, Some(1));
    }

    #[test]
    fn test_into_task_query_unknown_status_is_none() {
        let f = PluginTaskFilter {
            status: Some("archived".into()),
            ..Default::default()
        };
        let q = into_task_query(&f);
        assert_eq!(q.status, None);
    }

    #[test]
    fn test_into_task_query_category_id() {
        let f = PluginTaskFilter {
            category_id: Some(3),
            ..Default::default()
        };
        let q = into_task_query(&f);
        assert_eq!(q.category_id, Some(3));
    }

    // ─── PluginTaskView::from(Task) 脱敏 ──────

    #[test]
    fn test_plugin_task_view_strips_internal_fields() {
        let task = make_task();
        let v = PluginTaskView::from(task);
        assert_eq!(v.id, 1);
        assert_eq!(v.title, "测试任务");
        assert_eq!(v.status, "pending");
        assert_eq!(v.priority, 0);
        assert!(v.important);
        assert_eq!(v.due_at.unwrap(), "2026-06-07 10:00:00");
        // 核心断言：脱敏对象不暴露内部字段
        // source_batch_id / reminded_at 不在 PluginTaskView 结构体中
        // 通过 from() 转换确保了这一点
    }

    #[test]
    fn test_plugin_task_view_status_completed() {
        let mut task = make_task();
        task.status = 1;
        let v = PluginTaskView::from(task);
        assert_eq!(v.status, "completed");
    }

    #[test]
    fn test_plugin_task_view_archived() {
        let mut task = make_task();
        task.status = 2;
        let v = PluginTaskView::from(task);
        assert_eq!(v.status, "archived");
    }

    // ─── validate_repeat ──────────────────────

    #[test]
    fn test_validate_repeat_valid_daily() {
        assert!(validate_repeat(Some("daily"), Some(1), None, None).is_ok());
    }

    #[test]
    fn test_validate_repeat_invalid_kind() {
        assert!(validate_repeat(Some("yearly"), None, None, None).is_err());
    }

    #[test]
    fn test_validate_repeat_invalid_interval_zero() {
        assert!(validate_repeat(Some("daily"), Some(0), None, None).is_err());
    }

    #[test]
    fn test_validate_repeat_invalid_weekday() {
        assert!(validate_repeat(Some("weekly"), Some(1), Some("8"), None).is_err());
    }

    #[test]
    fn test_validate_repeat_valid_weekdays() {
        assert!(validate_repeat(Some("weekly"), Some(1), Some("1,3,5"), None).is_ok());
    }

    #[test]
    fn test_validate_repeat_count_zero() {
        assert!(validate_repeat(Some("daily"), Some(1), None, Some(0)).is_err());
    }
}
