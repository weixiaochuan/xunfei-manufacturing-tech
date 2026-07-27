import { useState } from "react";
import { Button, Dropdown, Space, Modal, message } from "antd";
import type { MenuProps } from "antd";
import { useNavigate } from "react-router-dom";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, ChevronDown, FileText } from "lucide-react";
import Markdown from "react-markdown";

import { FileTypeIcon } from "./FileTypeIcon";
import { ImportPreviewModal } from "./ImportPreviewModal";
import { importApi, sourceFileApi } from "@/lib/api";
import { useAppStore } from "@/store";
import type { ScannedFile } from "@/types";
import {
  importExcelFlow,
  importPdfsFlow,
  importTextFlow,
  importWordFlow,
} from "@/lib/noteCreator";

interface Props {
  /** 侧边栏折叠态 */
  collapsed?: boolean;
  /** 块级占满父容器宽度 */
  block?: boolean;
  /** 外层样式扩展 */
  style?: React.CSSProperties;
}

/**
 * "打开文件"分段按钮：主按钮预览打开 .md 文件，▼ 下拉承载其他导入入口。
 *
 * 主按钮：选 .md 文件 → 预览内容 → 确认导入
 * 下拉项：直接导入、导入 Markdown/TXT、PDF、Word、Excel、Markdown 文件夹
 */
export function OpenFileButton({
  collapsed = false,
  block = false,
  style,
}: Props) {
  const navigate = useNavigate();

  // 单文件预览
  const [previewOpen, setPreviewOpen] = useState(false);
  const [previewContent, setPreviewContent] = useState("");
  const [previewFileName, setPreviewFileName] = useState("");
  const [pendingPath, setPendingPath] = useState<string | null>(null);

  // 文件夹导入预览
  const [importPreview, setImportPreview] = useState<{
    files: ScannedFile[];
    rootPath: string;
  } | null>(null);

  /** 预览打开：读文件内容 → 弹预览 Modal → 确认后导入 */
  async function handlePreviewOpen() {
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
      });
      const path = Array.isArray(picked) ? picked[0] : picked;
      if (!path) return;
      const fileName = path.split(/[\\/]/).pop() ?? "未命名";
      setPreviewFileName(fileName);
      setPendingPath(path);

      // 读文件内容用于预览
      try {
        const base64 = await sourceFileApi.readFileAsBase64(path);
        const decoded = atob(base64);
        setPreviewContent(decoded);
      } catch {
        // 读失败时仍可预览
        setPreviewContent("*无法读取文件内容*");
      }
      setPreviewOpen(true);
    } catch (e) {
      message.error(`打开失败: ${e}`);
    }
  }

  /** 确认预览 → 导入 */
  async function confirmImport() {
    if (!pendingPath) return;
    setPreviewOpen(false);
    try {
      const result = await importApi.openMarkdownFile(pendingPath);
      if (result.wasSynced) {
        message.info("已根据最新 md 文件同步笔记内容");
      }
      useAppStore.getState().bumpNotesRefresh();
      navigate(`/notes/${result.noteId}`);
    } catch (e) {
      message.error(`导入失败: ${e}`);
    }
    setPendingPath(null);
    setPreviewContent("");
  }

  /** 取消预览 */
  function cancelPreview() {
    setPreviewOpen(false);
    setPendingPath(null);
    setPreviewContent("");
  }

  /** 直接导入（当前行为，跳过预览） */
  async function handleDirectOpen() {
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
      });
      const path = Array.isArray(picked) ? picked[0] : picked;
      if (!path) return;
      const result = await importApi.openMarkdownFile(path);
      if (result.wasSynced) {
        message.info("已根据最新 md 文件同步笔记内容");
      }
      useAppStore.getState().bumpNotesRefresh();
      navigate(`/notes/${result.noteId}`);
    } catch (e) {
      message.error(`打开失败: ${e}`);
    }
  }

  /** 导入 Markdown 文件夹 */
  async function handleImportMdFolder() {
    try {
      const picked = await openDialog({
        directory: true,
        title: "选择要导入的 Markdown 文件夹",
      });
      if (!picked || Array.isArray(picked)) return;
      const rootPath = picked;
      const hide = message.loading("扫描中…", 0);
      let files: ScannedFile[];
      try {
        files = await importApi.scan(rootPath);
      } catch (e) {
        hide();
        message.error(`扫描失败: ${e}`);
        return;
      }
      hide();
      if (files.length === 0) {
        message.info("该文件夹下没有 .md 文件");
        return;
      }
      setImportPreview({ files, rootPath });
    } catch (e) {
      message.error(`选择目录失败: ${e}`);
    }
  }

  const menuItems: MenuProps["items"] = [
    {
      key: "direct-open",
      icon: <FileText size={14} />,
      label: "直接导入…",
      onClick: handleDirectOpen,
    },
    { type: "divider" },
    {
      key: "import-text",
      icon: <FileTypeIcon type="md" size={14} />,
      label: "导入 Markdown / TXT…",
      onClick: () => importTextFlow(null, navigate),
    },
    {
      key: "import-md-folder",
      icon: <FolderOpen size={14} />,
      label: "导入 Markdown 文件夹…",
      onClick: () => {
        void handleImportMdFolder();
      },
    },
    {
      key: "import-pdf",
      icon: <FileTypeIcon type="pdf" size={14} />,
      label: "导入 PDF…",
      onClick: () => importPdfsFlow(null, navigate),
    },
    {
      key: "import-docx",
      icon: <FileTypeIcon type="docx" size={14} />,
      label: "导入 Word…",
      onClick: () => importWordFlow(null, navigate),
    },
    {
      key: "import-excel",
      icon: <FileTypeIcon type="xlsx" size={14} />,
      label: "导入 Excel…",
      onClick: () => importExcelFlow(null, navigate),
    },
  ];

  // 折叠态：只显示单图标按钮
  if (collapsed) {
    return (
      <>
        <Button
          icon={<FolderOpen size={16} />}
          onClick={handlePreviewOpen}
          title="打开文件"
          style={style}
        />
        <PreviewModal
          open={previewOpen}
          fileName={previewFileName}
          content={previewContent}
          onConfirm={confirmImport}
          onCancel={cancelPreview}
        />
        {importPreview && (
          <ImportPreviewModal
            open
            files={importPreview.files}
            rootPath={importPreview.rootPath}
            onCancel={() => setImportPreview(null)}
            onConfirm={async ({ policy, preserveRoot }) => {
              const { files, rootPath } = importPreview;
              setImportPreview(null);
              const paths = files.map((f) => f.path);
              const hide = message.loading(`正在导入 ${paths.length} 个文件…`, 0);
              try {
                const result = await importApi.importSelected(
                  paths,
                  null,
                  rootPath,
                  preserveRoot,
                  policy,
                );
                hide();
                // ... 结果提示
                if (result.imported > 0) message.success(`导入 ${result.imported} 篇`);
                if (result.errors.length > 0) message.warning(`${result.errors.length} 个文件失败`);
                useAppStore.getState().bumpNotesRefresh();
              } catch (e) {
                hide();
                message.error(`导入失败: ${e}`);
              }
            }}
          />
        )}
      </>
    );
  }

  return (
    <>
      <Space.Compact style={block ? { width: "100%", ...style } : style}>
        <Button
          icon={<FolderOpen size={14} />}
          onClick={handlePreviewOpen}
          title="预览打开 .md 文件"
          style={block ? { flex: 1 } : undefined}
        />
        <Dropdown
          menu={{ items: menuItems }}
          trigger={["click"]}
          placement="bottomRight"
        >
          <Button
            icon={<ChevronDown size={14} />}
            title="更多打开方式"
          />
        </Dropdown>
      </Space.Compact>

      {/* 预览 Modal */}
      <PreviewModal
        open={previewOpen}
        fileName={previewFileName}
        content={previewContent}
        onConfirm={confirmImport}
        onCancel={cancelPreview}
      />

      {/* 文件夹导入预览 */}
      {importPreview && (
        <ImportPreviewModal
          open
          files={importPreview.files}
          rootPath={importPreview.rootPath}
          onCancel={() => setImportPreview(null)}
          onConfirm={async ({ policy, preserveRoot }) => {
            const { files, rootPath } = importPreview;
            setImportPreview(null);
            const paths = files.map((f) => f.path);
            const hide = message.loading(`正在导入 ${paths.length} 个文件…`, 0);
            try {
              const result = await importApi.importSelected(
                paths,
                null,
                rootPath,
                preserveRoot,
                policy,
              );
              hide();
              if (result.imported > 0) message.success(`导入 ${result.imported} 篇`);
              if (result.skipped > 0) message.info(`跳过 ${result.skipped} 篇`);
              if (result.errors.length > 0) {
                message.warning(`${result.errors.length} 个文件失败，详见控制台`);
                console.warn("[import] 失败明细:", result.errors);
              }
              useAppStore.getState().bumpNotesRefresh();
              useAppStore.getState().bumpFoldersRefresh();
            } catch (e) {
              hide();
              message.error(`导入失败: ${e}`);
            }
          }}
        />
      )}
    </>
  );
}

/** 预览 Modal 子组件 */
function PreviewModal({
  open,
  fileName,
  content,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  fileName: string;
  content: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      open={open}
      title={
        <span className="flex items-center gap-2">
          <FileText size={16} />
          预览：{fileName}
        </span>
      }
      width={720}
      okText="导入"
      cancelText="取消"
      onOk={onConfirm}
      onCancel={onCancel}
      destroyOnClose
    >
      <div
        className="prose prose-sm max-w-none overflow-auto"
        style={{
          maxHeight: "60vh",
          padding: "12px 16px",
          borderRadius: 6,
          background: "var(--color-bg-elevated, #fafafa)",
          border: "1px solid var(--color-border-secondary, #e8e8e8)",
          fontSize: 14,
          lineHeight: 1.7,
        }}
      >
        <Markdown>{content}</Markdown>
      </div>
    </Modal>
  );
}
