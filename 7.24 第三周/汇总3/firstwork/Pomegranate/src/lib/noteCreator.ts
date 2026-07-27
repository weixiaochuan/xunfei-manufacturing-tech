import type { NavigateFunction } from "react-router-dom";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { message } from "antd";

import { importApi } from "@/lib/api";
import { useAppStore } from "@/store";

export async function createBlankAndOpen(
  _folderId?: number | null,
  navigate?: NavigateFunction,
  ..._rest: unknown[]
) {
  navigate?.("/notes");
}

export async function importTextFlow(
  folderId: number | null = null,
  navigate?: NavigateFunction,
) {
  let closeLoading: (() => void) | null = null;

  try {
    const picked = await openDialog({
      multiple: true,
      title: "选择要导入的 Markdown / TXT 文件",
      filters: [
        { name: "Markdown / Text", extensions: ["md", "markdown", "txt"] },
      ],
    });
    if (!picked) return null;

    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return null;

    closeLoading = message.loading(`正在导入 ${paths.length} 个文件…`, 0);
    const result = await importApi.importSelected(paths, folderId);
    closeLoading();
    closeLoading = null;

    const created = result.imported + result.duplicated;
    const existing = result.existingNoteIds?.length ?? 0;
    const parts: string[] = [];
    if (created > 0) parts.push(`导入 ${created} 篇`);
    if (existing > 0) parts.push(`命中已有 ${existing} 篇`);
    if (result.skipped > 0) parts.push(`跳过 ${result.skipped} 篇`);

    if (parts.length > 0) {
      message.success(parts.join("，"));
    } else if (result.errors.length === 0) {
      message.info("没有可导入的内容");
    }

    if (result.errors.length > 0) {
      const detail = result.errors[0] ?? "未知错误";
      if (created === 0 && existing === 0) {
        message.error(`导入失败：${detail}`);
      } else {
        message.warning(`${result.errors.length} 个文件导入失败，其他文件已完成`);
      }
      console.warn("[markdown-import] 失败明细:", result.errors);
    }

    const missingAttachments = result.attachments_missing?.length ?? 0;
    if (missingAttachments > 0) {
      message.warning(`${missingAttachments} 个本地附件未找到，已保留原始引用`);
    }

    useAppStore.getState().bumpNotesRefresh();
    useAppStore.getState().bumpFoldersRefresh();

    const firstNoteId = result.noteIds?.[0] ?? result.existingNoteIds?.[0];
    if (firstNoteId && navigate) {
      navigate(`/notes/${firstNoteId}`);
    }

    return result;
  } catch (error) {
    message.error(`导入 Markdown 失败：${String(error)}`);
    return null;
  } finally {
    closeLoading?.();
  }
}

async function importRecognizedFiles(
  title: string,
  extensions: string[],
  folderId: number | null,
  navigate?: NavigateFunction,
) {
  let closeLoading: (() => void) | null = null;
  try {
    const picked = await openDialog({
      multiple: true,
      title,
      filters: [{ name: "支持的文档", extensions }],
    });
    if (!picked) return null;

    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return null;

    closeLoading = message.loading(`正在识别并导入 ${paths.length} 个文件…`, 0);
    const result = await importApi.importMixed(paths, folderId);
    closeLoading();
    closeLoading = null;

    const parts: string[] = [];
    if (result.imported > 0) parts.push(`导入 ${result.imported} 篇`);
    if (result.duplicated > 0) parts.push(`副本 ${result.duplicated} 篇`);
    if (result.skipped > 0) parts.push(`跳过 ${result.skipped} 个`);
    if (parts.length > 0) message.success(parts.join("，"));

    if (result.errors.length > 0) {
      if (result.imported + result.duplicated === 0) {
        message.error(`导入失败：${result.errors[0]}`);
      } else {
        message.warning(`${result.errors.length} 个文件导入失败，其他文件已完成`);
      }
      console.warn("[document-import] 失败明细:", result.errors);
    }

    useAppStore.getState().bumpNotesRefresh();
    useAppStore.getState().bumpFoldersRefresh();
    const firstNoteId = result.noteIds?.[0] ?? result.existingNoteIds?.[0];
    if (firstNoteId && navigate) navigate(`/notes/${firstNoteId}`);
    return result;
  } catch (error) {
    message.error(`导入文档失败：${String(error)}`);
    return null;
  } finally {
    closeLoading?.();
  }
}

/** 选择多个不同格式的文件，Rust 后端按每个文件自身类型分别处理。 */
export async function importDocumentFilesFlow(
  folderId: number | null = null,
  navigate?: NavigateFunction,
) {
  return importRecognizedFiles(
    "选择要导入的文档（可混合选择）",
    ["md", "markdown", "txt", "pdf", "doc", "docx", "xlsx", "xls", "xlsm", "xlsb", "ods"],
    folderId,
    navigate,
  );
}

export async function importPdfsFlow(
  folderId: number | null = null,
  navigate?: NavigateFunction,
) {
  return importRecognizedFiles("选择要导入的 PDF", ["pdf"], folderId, navigate);
}

export async function importWordFlow(
  folderId: number | null = null,
  navigate?: NavigateFunction,
) {
  return importRecognizedFiles(
    "选择要导入的 Word 文档",
    ["doc", "docx"],
    folderId,
    navigate,
  );
}

export async function importExcelFlow(
  folderId: number | null = null,
  navigate?: NavigateFunction,
) {
  return importRecognizedFiles(
    "选择要导入的 Excel / 表格文件",
    ["xlsx", "xls", "xlsm", "xlsb", "ods"],
    folderId,
    navigate,
  );
}
