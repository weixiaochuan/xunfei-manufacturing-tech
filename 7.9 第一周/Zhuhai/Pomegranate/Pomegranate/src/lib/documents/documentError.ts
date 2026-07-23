const SAFE_MESSAGES: Record<string, string> = {
  unauthenticated: "请先登录",
  signedOut: "请先登录",
  account_service_unavailable: "账号服务暂不可用",
  unavailable: "账号服务暂不可用",
  document_conflict: "文档已在其他位置更新",
  documentConflict: "文档已在其他位置更新",
  not_found: "文档不存在或无权访问",
  notFound: "文档不存在或无权访问",
  validation_error: "文档信息不完整",
  validationError: "新建参数格式错误",
  invalid_title: "文档标题缺失",
  titleInvalid: "文档标题缺失",
  invalid_markdown_content: "文档正文格式无效",
  markdownContentInvalid: "文档正文格式无效",
  invalid_folder: "文件夹信息无效",
  folderInvalid: "文件夹信息无效",
  invalid_tag_ids: "标签信息无效",
  invalid_tags: "标签信息无效",
  tagsInvalid: "标签信息无效",
  requestShapeInvalid: "新建参数格式错误",
  invalidResponse: "账号服务返回了无效数据",
  network_error: "网络连接失败",
  staleRequest: "账号已切换，请重试",
  tooLarge: "文件超过允许大小",
  file_too_large: "文件超过允许大小",
  fileTypeRejected: "不支持上传此文件类型",
  file_type_rejected: "不支持上传此文件类型",
  uploadFailed: "文件上传失败，请稍后重试",
  markdownReadFailed: "无法读取所选文件",
  markdownEncodingUnsupported: "暂不支持该文件编码",
  markdownTooLarge: "Markdown 文件超过允许大小",
  openFailed: "无法使用系统默认程序打开文件",
};

const BLOCKED_DETAIL = /(?:bearer|token|password|secret|postgres|sql|stack|https?:\/\/|[a-z]:\\)/i;

function objectText(value: Record<string, unknown>): string | null {
  for (const key of ["code", "error", "kind"]) {
    const code = value[key];
    if (typeof code === "string" && SAFE_MESSAGES[code]) return SAFE_MESSAGES[code];
  }
  for (const key of ["message", "detail"]) {
    const detail = value[key];
    if (typeof detail === "string" && detail.trim() && !BLOCKED_DETAIL.test(detail)) {
      return detail.trim();
    }
  }
  return null;
}

/** Convert any Tauri/server error into one safe user-facing string. */
export function documentErrorMessage(error: unknown, fallback = "操作失败，请稍后重试"): string {
  if (typeof error === "string") {
    const trimmed = error.trim();
    if (SAFE_MESSAGES[trimmed]) return SAFE_MESSAGES[trimmed];
    return trimmed && !BLOCKED_DETAIL.test(trimmed) ? trimmed : fallback;
  }
  if (error instanceof Error) return documentErrorMessage(error.message, fallback);
  if (error && typeof error === "object") {
    return objectText(error as Record<string, unknown>) ?? fallback;
  }
  return fallback;
}
