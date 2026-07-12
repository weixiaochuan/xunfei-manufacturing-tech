use std::fs;
use std::path::Path;

use regex::Regex;

use crate::error::AppError;
use crate::models::session::{ParsedPlan, PhaseDraft};

/// 计划文件解析服务
pub struct SessionPlanService;

impl SessionPlanService {
    /// 解析 Markdown 计划文件，提取 Phase 列表
    ///
    /// 支持的格式：
    /// - `## Phase N: 名称` / `## Phase N：名称`（Markdown 标题）
    /// - `### Phase N: 名称`（三级标题）
    /// - `Phase N: 名称`（纯文本行，以 "Phase" 开头）
    /// - `Phase N：名称`（中文冒号）
    /// - `步骤 N: 名称` / `Step N: 名称`
    /// - `任务 N: 名称` / `Task N: 名称`
    pub fn parse_plan_file(path: &str) -> Result<ParsedPlan, AppError> {
        let file_path = Path::new(path);

        if !file_path.exists() {
            return Err(AppError::NotFound(format!("计划文件不存在: {}", path)));
        }

        let content = fs::read_to_string(file_path).map_err(|e| {
            AppError::Custom(format!("无法读取计划文件: {}", e))
        })?;

        // 提取计划名称：取第一个 # 标题，或文件名
        let name = Self::extract_plan_name(&content, file_path);

        // 解析 Phase
        let phases = Self::extract_phases(&content);

        Ok(ParsedPlan { name, phases })
    }

    /// 从 Markdown 内容提取计划名称
    fn extract_plan_name(content: &str, file_path: &Path) -> String {
        // 尝试取第一个 # 标题
        if let Some(line) = content.lines().find(|l| l.starts_with("# ") || l.starts_with("# ")) {
            let name = line.trim_start_matches('#').trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
        // 回退用文件名（去掉扩展名）
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名计划")
            .to_string()
    }

    /// 从 Markdown 内容提取 Phase 列表
    fn extract_phases(content: &str) -> Vec<PhaseDraft> {
        let mut phases: Vec<PhaseDraft> = Vec::new();

        // 标题级 Phase 匹配: ## Phase N: 名称 / ### Phase N: 名称
        let title_re = Regex::new(r"(?i)^#{1,5}\s*phase\s*(\d+)\s*[:：]\s*(.+)").unwrap();
        // 纯文本 Phase 匹配: Phase N: 名称 / Phase N：名称
        let text_re = Regex::new(r"(?i)^phase\s*(\d+)\s*[:：]\s*(.+)").unwrap();
        // 步骤匹配: 步骤 N: 名称 / Step N: 名称
        let step_re = Regex::new(r"(?i)^(?:步骤|step)\s*(\d+)\s*[:：]\s*(.+)").unwrap();
        // 任务匹配: 任务 N: 名称 / Task N: 名称
        let task_re = Regex::new(r"(?i)^(?:任务|task)\s*(\d+)\s*[:：]\s*(.+)").unwrap();

        // 收集描述文本
        let mut current_desc_lines: Vec<String> = Vec::new();
        let mut pending_phase: Option<PhaseDraft> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // 检查是否为新 Phase 行
            let mut matched = false;
            let mut phase_idx: u32 = 0;
            let mut phase_name = String::new();

            if let Some(caps) = title_re.captures(trimmed) {
                phase_idx = caps[1].parse().unwrap_or(0);
                phase_name = caps[2].trim().to_string();
                matched = true;
            } else if let Some(caps) = text_re.captures(trimmed) {
                phase_idx = caps[1].parse().unwrap_or(0);
                phase_name = caps[2].trim().to_string();
                matched = true;
            } else if let Some(caps) = step_re.captures(trimmed) {
                phase_idx = caps[1].parse().unwrap_or(0);
                phase_name = caps[2].trim().to_string();
                matched = true;
            } else if let Some(caps) = task_re.captures(trimmed) {
                phase_idx = caps[1].parse().unwrap_or(0);
                phase_name = caps[2].trim().to_string();
                matched = true;
            }

            if matched {
                // 保存上一个 Phase（带描述）
                if let Some(mut prev) = pending_phase.take() {
                    if !current_desc_lines.is_empty() {
                        prev.description = current_desc_lines.join(" ");
                    }
                    phases.push(prev);
                    current_desc_lines.clear();
                }

                // 创建新 Phase
                pending_phase = Some(PhaseDraft {
                    id: format!("phase_{}", phase_idx),
                    name: phase_name,
                    description: String::new(),
                });
            } else if pending_phase.is_some() {
                // 当前 Phase 的描述行
                // 跳过标题和空行，收集描述
                if !trimmed.starts_with('#') && trimmed.len() > 1 {
                    current_desc_lines.push(trimmed.to_string());
                }
            }
        }

        // 保存最后一个 Phase
        if let Some(mut prev) = pending_phase.take() {
            if !current_desc_lines.is_empty() {
                prev.description = current_desc_lines.join(" ");
            }
            phases.push(prev);
        }

        // 去重：按 id 保留第一个
        let mut seen = std::collections::HashSet::new();
        phases.retain(|p| seen.insert(p.id.clone()));

        phases
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_phases() {
        let content = r#"
# 测试计划

## Phase 0: 环境准备
安装必要的工具和依赖

## Phase 1: 核心开发
实现主要功能模块
包含单元测试

### Phase 2: 测试
全面测试
"#;
        let phases = SessionPlanService::extract_phases(content);
        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0].id, "phase_0");
        assert_eq!(phases[0].name, "环境准备");
        assert!(phases[0].description.contains("安装必要的工具"));
        assert_eq!(phases[1].id, "phase_1");
        assert_eq!(phases[1].name, "核心开发");
        assert_eq!(phases[2].id, "phase_2");
        assert_eq!(phases[2].name, "测试");
    }

    #[test]
    fn test_extract_phases_text_format() {
        let content = r#"
Phase 0: 初始化项目
Phase 1: 数据库设计
Phase 2: API开发
"#;
        let phases = SessionPlanService::extract_phases(content);
        assert_eq!(phases.len(), 3);
    }

    #[test]
    fn test_extract_plan_name_from_filename() {
        let path = std::path::Path::new("/tmp/test_plan.md");
        let name = SessionPlanService::extract_plan_name("", path);
        assert_eq!(name, "test_plan");
    }
}
