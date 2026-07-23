import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Empty,
  Popconfirm,
  Space,
  Spin,
  Table,
  Typography,
  message,
  type TableColumnsType,
} from "antd";
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  LoginOutlined,
  UploadOutlined,
} from "@ant-design/icons";
import dayjs from "dayjs";
import {
  accountFilesApi,
  type AccountFileCommandError,
  type AccountUserFile,
} from "@/lib/api";
import { useAccountStore } from "@/store/account";
import { formatFileSize } from "./file-utils";

const { Title, Text } = Typography;

function normalizeCommandError(error: unknown): AccountFileCommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.code === "string" &&
    typeof error.message === "string"
  ) {
    return { code: error.code, message: error.message };
  }
  return { code: "unknown", message: "文件操作失败，请稍后重试" };
}

export default function AccountFilesPage() {
  const { currentUser, loginStatus, beginLogin, applyLoginResult } = useAccountStore();
  const [loading, setLoading] = useState(false);
  const [files, setFiles] = useState<AccountUserFile[]>([]);
  const [uploading, setUploading] = useState(false);
  const [downloadingFileId, setDownloadingFileId] = useState<string | null>(null);
  const [deletingFileId, setDeletingFileId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleError = useCallback(
    (caught: unknown, fallback: string) => {
      const commandError = normalizeCommandError(caught);
      if (commandError.code === "signedOut") {
        applyLoginResult({ status: "signedOut" });
        setError("登录已失效，请重新登录");
        return;
      }
      if (commandError.code === "unavailable") {
        setError("账号服务暂不可用");
        return;
      }
      setError(commandError.message || fallback);
      message.error(commandError.message || fallback);
    },
    [applyLoginResult],
  );

  const loadFiles = useCallback(async () => {
    if (!currentUser) return;
    setLoading(true);
    setError(null);
    try {
      const result = await accountFilesApi.list();
      setFiles(result.files);
    } catch (caught) {
      handleError(caught, "加载文件列表失败");
    } finally {
      setLoading(false);
    }
  }, [currentUser, handleError]);

  useEffect(() => {
    if (currentUser) {
      void loadFiles();
    } else {
      setFiles([]);
    }
  }, [currentUser, loadFiles]);

  const busy = uploading || downloadingFileId !== null || deletingFileId !== null;

  const handleUpload = async () => {
    setUploading(true);
    setError(null);
    try {
      const result = await accountFilesApi.pickAndUpload();
      if (result.status === "cancelled") return;
      message.success(`已上传 ${result.file.originalName}`);
      await loadFiles();
    } catch (caught) {
      handleError(caught, "上传文件失败");
    } finally {
      setUploading(false);
    }
  };

  const handleDownload = async (file: AccountUserFile) => {
    setDownloadingFileId(file.id);
    setError(null);
    try {
      const result = await accountFilesApi.download(file.id);
      if (result.status === "cancelled") return;
      message.success(`已保存 ${result.fileName}`);
    } catch (caught) {
      handleError(caught, "下载文件失败");
    } finally {
      setDownloadingFileId(null);
    }
  };

  const handleDelete = async (file: AccountUserFile) => {
    setDeletingFileId(file.id);
    setError(null);
    try {
      await accountFilesApi.remove(file.id);
      setFiles((current) => current.filter((item) => item.id !== file.id));
      message.success(`已删除 ${file.originalName}`);
    } catch (caught) {
      handleError(caught, "删除文件失败");
    } finally {
      setDeletingFileId(null);
    }
  };

  const columns = useMemo<TableColumnsType<AccountUserFile>>(
    () => [
      { title: "文件名", dataIndex: "originalName", key: "originalName", ellipsis: true },
      {
        title: "大小",
        dataIndex: "sizeBytes",
        key: "sizeBytes",
        width: 110,
        render: (value: number) => formatFileSize(value),
      },
      {
        title: "类型",
        dataIndex: "mimeType",
        key: "mimeType",
        width: 180,
        ellipsis: true,
        render: (value: string | null) => value || "未知",
      },
      {
        title: "上传时间",
        dataIndex: "createdAt",
        key: "createdAt",
        width: 180,
        render: (value: string) => dayjs(value).format("YYYY-MM-DD HH:mm"),
      },
      {
        title: "操作",
        key: "actions",
        width: 190,
        render: (_, file) => (
          <Space>
            <Button
              type="link"
              icon={<CloudDownloadOutlined />}
              loading={downloadingFileId === file.id}
              disabled={busy && downloadingFileId !== file.id}
              onClick={() => void handleDownload(file)}
            >
              下载
            </Button>
            <Popconfirm
              title="删除这个文件？"
              description="删除后无法从账号文件中恢复。"
              okText="删除"
              cancelText="取消"
              okButtonProps={{ danger: true }}
              disabled={busy}
              onConfirm={() => handleDelete(file)}
            >
              <Button
                type="link"
                danger
                icon={<DeleteOutlined />}
                loading={deletingFileId === file.id}
                disabled={busy && deletingFileId !== file.id}
              >
                删除
              </Button>
            </Popconfirm>
          </Space>
        ),
      },
    ],
    [busy, deletingFileId, downloadingFileId],
  );

  if (!currentUser) {
    const restoring = loginStatus === "restoring";
    return (
      <div className="h-full p-6">
        <Card>
          {restoring ? (
            <div className="flex min-h-48 items-center justify-center">
              <Spin tip="正在恢复账号" />
            </div>
          ) : (
            <Empty description="请先登录">
              <Button type="primary" icon={<LoginOutlined />} onClick={() => void beginLogin()}>
                登录
              </Button>
            </Empty>
          )}
        </Card>
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto p-6">
      <div className="mx-auto max-w-6xl">
        <div className="mb-5 flex flex-wrap items-start justify-between gap-4">
          <div>
            <Title level={2} style={{ marginBottom: 4 }}>我的文件</Title>
            <Text type="secondary">文件仅当前账号可见</Text>
          </div>
          <Button
            type="primary"
            icon={<UploadOutlined />}
            loading={uploading}
            disabled={busy && !uploading}
            onClick={() => void handleUpload()}
          >
            上传文件
          </Button>
        </div>

        {error && (
          <Alert
            className="mb-4"
            type="warning"
            showIcon
            message={error}
            action={<Button size="small" onClick={() => void loadFiles()}>重试</Button>}
          />
        )}

        <Card styles={{ body: { padding: 0 } }}>
          <Table<AccountUserFile>
            rowKey="id"
            columns={columns}
            dataSource={files}
            loading={loading}
            pagination={false}
            scroll={{ x: 780 }}
            locale={{
              emptyText: (
                <Empty description="还没有文件，上传第一个文件吧" image={Empty.PRESENTED_IMAGE_SIMPLE} />
              ),
            }}
          />
        </Card>
      </div>
    </div>
  );
}
