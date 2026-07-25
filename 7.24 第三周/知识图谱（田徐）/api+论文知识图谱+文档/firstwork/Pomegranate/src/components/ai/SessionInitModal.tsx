import { useState } from "react";
import { Modal, Steps, Input, Button, Upload, List, Tag, App, Typography } from "antd";
import { FileTextOutlined, InboxOutlined } from "@ant-design/icons";
import { useNavigate } from "react-router-dom";
import { sessionApi } from "@/lib/api";
import { useAppStore } from "@/store";
import type { ParsedPlan } from "@/types";

const { Text } = Typography;
const { TextArea } = Input;
const { Dragger } = Upload;

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function SessionInitModal({ open: visible, onClose }: Props) {
  const { message } = App.useApp();
  const navigate = useNavigate();
  const setActiveSession = useAppStore((s) => s.setActiveSession);

  const [currentStep, setCurrentStep] = useState(0);
  const [planPath, setPlanPath] = useState("");
  const [pasteContent, setPasteContent] = useState("");
  const [parsedPlan, setParsedPlan] = useState<ParsedPlan | null>(null);
  const [parsing, setParsing] = useState(false);
  const [creating, setCreating] = useState(false);
  const [mode, setMode] = useState<"file" | "paste">("file");

  async function handleParse() {
    setParsing(true);
    try {
      let path = planPath;
      if (mode === "paste" && pasteContent.trim()) {
        // 将粘贴内容保存为临时文件
        const { invoke } = await import("@tauri-apps/api/core");
        path = await invoke<string>("save_temp_plan", { content: pasteContent.trim() });
      }
      if (!path) {
        message.error("请先选择计划文件或粘贴内容");
        return;
      }
      const plan = await sessionApi.parsePlan(path);
      setParsedPlan(plan);
      setCurrentStep(1);
    } catch (e) {
      message.error(`解析失败: ${e}`);
    } finally {
      setParsing(false);
    }
  }

  async function handleCreate() {
    if (!planPath || creating) return;

    // 构造完整路径
    let path = planPath;

    setCreating(true);
    try {
      if (mode === "paste" && !path) {
        // Already saved as temp by parse step
        const { invoke } = await import("@tauri-apps/api/core");
        path = await invoke<string>("save_temp_plan", { content: pasteContent.trim() });
      }
      const session = await sessionApi.create(path);
      setActiveSession(session);
      message.success("会话创建成功");
      onClose();
      navigate(`/task-session?sessionId=${session.id}`);
    } catch (e) {
      message.error(`创建失败: ${e}`);
    } finally {
      setCreating(false);
    }
  }

  function handleClose() {
    setCurrentStep(0);
    setPlanPath("");
    setPasteContent("");
    setParsedPlan(null);
    setMode("file");
    onClose();
  }

  return (
    <Modal
      title="新建任务执行会话"
      open={visible}
      onCancel={handleClose}
      width={640}
      footer={null}
      destroyOnClose
    >
      <Steps
        current={currentStep}
        size="small"
        className="mb-6"
        items={[
          { title: "选择计划" },
          { title: "预览确认" },
        ]}
      />

      {currentStep === 0 && (
        <div className="space-y-4">
          {/* 模式切换 */}
          <div className="flex gap-2">
            <Tag.CheckableTag
              checked={mode === "file"}
              onChange={() => setMode("file")}
            >
              选择文件
            </Tag.CheckableTag>
            <Tag.CheckableTag
              checked={mode === "paste"}
              onChange={() => setMode("paste")}
            >
              粘贴内容
            </Tag.CheckableTag>
          </div>

          {mode === "file" ? (
            <Dragger
              accept=".md,.txt"
              multiple={false}
              beforeUpload={(file: File & { path?: string }) => {
                setPlanPath(file.path ?? "");
                return false;
              }}
              showUploadList={false}
            >
              <div className="p-4">
                <InboxOutlined className="text-2xl" />
                <p className="text-sm mt-2">点击选择或拖拽计划文件</p>
                <p className="text-xs text-gray-400">支持 .md 和 .txt 文件</p>
              </div>
            </Dragger>
          ) : (
            <TextArea
              value={pasteContent}
              onChange={(e) => setPasteContent(e.target.value)}
              placeholder={`请粘贴计划内容，例如：

# 开发计划

## Phase 0: 环境准备
安装开发工具和依赖

## Phase 1: 核心开发
实现主要功能模块

## Phase 2: 测试验收
全面测试和修复`}
              rows={10}
            />
          )}

          {mode === "file" && planPath && (
            <Text type="secondary" className="text-xs block truncate">
              <FileTextOutlined className="mr-1" />
              {planPath}
            </Text>
          )}

          <div className="flex justify-end gap-2">
            <Button onClick={handleClose}>取消</Button>
            <Button type="primary" onClick={handleParse} loading={parsing}>
              解析计划
            </Button>
          </div>
        </div>
      )}

      {currentStep === 1 && parsedPlan && (
        <div className="space-y-4">
          <div>
            <Text strong>计划名称：</Text>
            <Text>{parsedPlan.name}</Text>
          </div>
          <div>
            <Text strong>阶段数：</Text>
            <Tag color="blue">{parsedPlan.phases.length}</Tag>
          </div>
          <List
            size="small"
            dataSource={parsedPlan.phases}
            renderItem={(phase) => (
              <List.Item>
                <div className="flex items-center gap-2">
                  <Tag>{phase.id}</Tag>
                  <Text strong>{phase.name}</Text>
                  {phase.description && (
                    <Text type="secondary" className="text-xs truncate max-w-[300px]">
                      — {phase.description}
                    </Text>
                  )}
                </div>
              </List.Item>
            )}
          />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setCurrentStep(0)}>返回</Button>
            <Button type="primary" onClick={handleCreate} loading={creating}>
              创建会话
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
