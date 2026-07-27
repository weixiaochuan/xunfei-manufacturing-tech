import { useEffect, useState } from "react";
import { Card, Typography, Descriptions, Spin, message, Button, Tooltip } from "antd";
import { SyncOutlined } from "@ant-design/icons";
import { FolderOpen, ExternalLink } from "lucide-react";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import type { SystemInfo } from "@/types";
import { systemApi, updaterApi } from "@/lib/api";
import { RecommendCards } from "@/components/ui/RecommendCards";
import { UpdateModal } from "@/components/ui/UpdateModal";

const OFFICIAL_SITE = "https://kb.ruoyi.plus/";
const QQ_GROUP_ID = "897442341";
const AUTHOR_QQ = "3956643";
const AUTHOR_WECHAT = "Wen_Jing_Qian";

const { Title, Text } = Typography;

export interface AboutContentProps {
  /** 显示页面标题 + 副标题 */
  showHeader?: boolean;
  /** 显示"前往设置"按钮 */
  showNavigateToSettings?: boolean;
  /** 显示赞赏支持区块 */
  showSponsor?: boolean;
  /** 显示推荐应用区块 */
  showRecommend?: boolean;
  /** 用于导航到设置页的回调（设置页内使用时不传，关于页需要传） */
  onNavigateSettings?: () => void;
  /** 锚点 id 前缀，避免与设置页 id 冲突 */
  idPrefix?: string;
}

export function AboutContent(props: AboutContentProps) {
  const {
    showHeader = true,
    showNavigateToSettings = false,
    showSponsor = true,
    showRecommend = true,
    onNavigateSettings,
    idPrefix = "about",
  } = props;

  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [updateModalOpen, setUpdateModalOpen] = useState(false);

  useEffect(() => {
    systemApi
      .getSystemInfo()
      .then(setInfo)
      .catch((e) => message.error(String(e)))
      .finally(() => setLoading(false));
  }, []);

  async function handleOpenDataDir() {
    if (!info?.dataDir) return;
    try {
      await openPath(info.dataDir);
    } catch (e) {
      message.error(`打开目录失败: ${e}`);
    }
  }

  async function handleCheckUpdate() {
    setChecking(true);
    try {
      const result = await updaterApi.checkUpdate();
      if (result) {
        setUpdate(result);
        setUpdateModalOpen(true);
      } else {
        message.success("当前已是最新版本");
      }
    } catch (e) {
      message.warning(`检查更新失败: ${String(e)}`);
    } finally {
      setChecking(false);
    }
  }

  return (
    <>
      {showHeader && (
        <div className="flex items-start justify-between gap-2">
          <div>
            <Title level={3} style={{ marginBottom: 4 }}>关于</Title>
            <Text type="secondary">系统信息和应用版本</Text>
          </div>
          {showNavigateToSettings && onNavigateSettings && (
            <Button onClick={onNavigateSettings}>前往设置</Button>
          )}
        </div>
      )}

      <Card id={`${idPrefix}-system`} title="系统信息">
        {loading ? (
          <div className="flex justify-center py-8">
            <Spin />
          </div>
        ) : info ? (
          <Descriptions column={1} bordered size="small">
            <Descriptions.Item label="操作系统">{info.os}</Descriptions.Item>
            <Descriptions.Item label="CPU 架构">{info.arch}</Descriptions.Item>
            <Descriptions.Item label="应用版本">
              <div className="flex items-center justify-between gap-2">
                <Text style={{ fontSize: 13 }}>v{info.appVersion}</Text>
                <Button
                  type="link"
                  size="small"
                  icon={<SyncOutlined spin={checking} />}
                  loading={checking}
                  onClick={handleCheckUpdate}
                >
                  检查更新
                </Button>
              </div>
            </Descriptions.Item>
            <Descriptions.Item label="官网">
              <div className="flex items-center justify-between gap-2">
                <Text style={{ fontSize: 13 }}>{OFFICIAL_SITE}</Text>
                <Tooltip title="在浏览器中打开">
                  <Button
                    type="link"
                    size="small"
                    icon={<ExternalLink size={14} />}
                    onClick={() => openUrl(OFFICIAL_SITE)}
                  />
                </Tooltip>
              </div>
            </Descriptions.Item>
            <Descriptions.Item label="数据目录">
              <div className="flex items-center justify-between gap-2">
                <Text copyable={{ text: info.dataDir }} style={{ fontSize: 13 }}>
                  {info.dataDir}
                </Text>
                <Tooltip title="在文件管理器中打开">
                  <Button
                    type="link"
                    size="small"
                    icon={<FolderOpen size={14} />}
                    onClick={handleOpenDataDir}
                  />
                </Tooltip>
              </div>
            </Descriptions.Item>
          </Descriptions>
        ) : (
          <Text type="danger">无法获取系统信息</Text>
        )}
      </Card>

      <Card id={`${idPrefix}-community`} title="作者 & 社区">
        <Descriptions column={1} bordered size="small">
          <Descriptions.Item label="QQ 交流群">
            <div className="flex items-center gap-2 flex-wrap">
              <Text style={{ fontSize: 13 }}>群号</Text>
              <Text copyable={{ text: QQ_GROUP_ID }} strong style={{ fontSize: 13 }}>
                {QQ_GROUP_ID}
              </Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                （Bug 反馈 / 使用交流 / 新功能讨论）
              </Text>
            </div>
          </Descriptions.Item>
          <Descriptions.Item label="联系作者">
            <div className="flex items-center gap-2 flex-wrap">
              <Text style={{ fontSize: 13 }}>QQ</Text>
              <Tooltip title="点击复制">
                <Text copyable={{ text: AUTHOR_QQ }} strong style={{ fontSize: 13 }}>
                  {AUTHOR_QQ}
                </Text>
              </Tooltip>
              <Text type="secondary" style={{ fontSize: 12 }}>/</Text>
              <Text style={{ fontSize: 13 }}>微信</Text>
              <Tooltip title="点击复制">
                <Text copyable={{ text: AUTHOR_WECHAT }} strong style={{ fontSize: 13 }}>
                  {AUTHOR_WECHAT}
                </Text>
              </Tooltip>
              <Text type="secondary" style={{ fontSize: 12 }}>
                （添加时请备注「来自知识库」）
              </Text>
            </div>
          </Descriptions.Item>
        </Descriptions>
      </Card>

      {showSponsor && (
        <Card id={`${idPrefix}-sponsor`} title="赞赏支持">
          <div className="flex items-center gap-6 flex-wrap">
            <img
              src="/donate-qr.png"
              alt="赞赏码"
              style={{
                width: 200,
                height: 200,
                objectFit: "contain",
                borderRadius: 8,
                background: "#fff",
                padding: 4,
                border: "1px solid #f0f0f0",
              }}
            />
            <div className="flex-1 min-w-[200px]">
              <Title level={5} style={{ marginTop: 0 }}>
                如果这款工具帮到了你 ❤️
              </Title>
              <Typography.Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 8 }}>
                本应用完全开源免费、无任何会员/订阅。如果觉得对你有用，欢迎扫描左侧
                微信赞赏码请作者喝杯咖啡 ☕，能让我有更多动力继续投入。
              </Typography.Paragraph>
              <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
                💡 不想赞赏也欢迎：在 B 站点个关注 / 在 GitHub 给项目点 Star /
                把它推荐给身边需要的朋友。
              </Typography.Paragraph>
            </div>
          </div>
        </Card>
      )}

      {info && (
        <Card
          id={`${idPrefix}-migration`}
          title="数据迁移说明"
          size="small"
        >
          <Typography.Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 10 }}>
            按使用场景从简单到专业，推荐 4 种方式：
          </Typography.Paragraph>

          <Typography.Title level={5} style={{ fontSize: 13, marginBottom: 4, marginTop: 0 }}>
            ① 单台电脑换硬盘 / 搬到 D 盘
          </Typography.Title>
          <Typography.Paragraph style={{ fontSize: 13, marginBottom: 12 }}>
            <Text strong>设置 → 数据目录</Text> 选新路径，勾选「
            <Text type="success">自动迁移</Text>
            」即可。应用会启动迁移引导窗口完成搬运，无需手工复制文件。
          </Typography.Paragraph>

          <Typography.Title level={5} style={{ fontSize: 13, marginBottom: 4, marginTop: 0 }}>
            ② 一次性整包搬到另一台电脑（离线）
          </Typography.Title>
          <Typography.Paragraph style={{ fontSize: 13, marginBottom: 12 }}>
            旧电脑：<Text strong>设置 → 同步 → 本地 ZIP → 导出</Text>{" "}
            得到一个 .zip 快照（含全部数据库 + 图片 + PDF + 附件 + 源文件）。
            新电脑安装应用后到同位置选择 <Text strong>导入 ZIP</Text>，自动解压覆盖。
          </Typography.Paragraph>

          <Typography.Title level={5} style={{ fontSize: 13, marginBottom: 4, marginTop: 0 }}>
            ③ 多端实时双向同步（推荐长期用户）
          </Typography.Title>
          <Typography.Paragraph style={{ fontSize: 13, marginBottom: 12 }}>
            <Text strong>设置 → 同步 → 多端同步（V1）</Text>{" "}
            配置 WebDAV / 坚果云 / NAS 后端；多台电脑都登录同一个账号，应用会按文件级
            manifest 增量推拉，自动消化双端冲突。也可以只用「
            <Text>WebDAV 全量快照</Text>」做单向手动备份。
          </Typography.Paragraph>

          <Typography.Title level={5} style={{ fontSize: 13, marginBottom: 4, marginTop: 0 }}>
            ④ 手动复制（兜底方案，应急用）
          </Typography.Title>
          <Typography.Paragraph
            type="secondary"
            style={{ fontSize: 12, marginBottom: 6 }}
          >
            数据目录下的核心文件 / 子目录：
          </Typography.Paragraph>
          <ul style={{ fontSize: 12, paddingLeft: 20, margin: "0 0 8px", color: "rgba(0,0,0,0.45)" }}>
            <li style={{ marginBottom: 2 }}><code>app.db</code> — 笔记 / 文件夹 / 标签 / 链接 / AI 对话 / 待办 / 加密数据等全部元数据（SQLite）</li>
            <li style={{ marginBottom: 2 }}><code>kb_assets/</code> — 笔记内嵌图片（含 <code>kb_assets/videos/</code> 子目录的视频）</li>
            <li style={{ marginBottom: 2 }}><code>pdfs/</code> — 导入的 PDF 原始文件</li>
            <li style={{ marginBottom: 2 }}><code>sources/</code> — 导入的 Word/Excel 原始文件</li>
            <li style={{ marginBottom: 2 }}><code>attachments/</code> — 笔记附件（zip / 音频等通用文件）</li>
            <li><code>settings.json</code> — 应用偏好（主题、窗口状态、字体等）</li>
          </ul>
          <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
            步骤：关闭应用 → 整目录复制到新电脑相同路径（点上方「打开数据目录」定位）→
            启动即可。<Text strong>务必整目录一起搬</Text>，单独复制 <code>app.db</code>{" "}
            会丢图片 / PDF / 附件。
          </Typography.Paragraph>

          <Typography.Paragraph
            type="warning"
            style={{ fontSize: 12, marginTop: 12, marginBottom: 0 }}
          >
            ⚠ 任何方式都要在迁移前关闭应用；新旧两端版本号差距不要超过一个小版本，避免
            schema 不兼容。需要给其他工具用，可在
            <Text strong> 设置 → 导出 Markdown</Text> 单独导出标准 .md 文件。
          </Typography.Paragraph>
        </Card>
      )}

      {showRecommend && (
        <div id={`${idPrefix}-recommend`}>
          <RecommendCards />
        </div>
      )}

      <UpdateModal
        open={updateModalOpen}
        onClose={() => setUpdateModalOpen(false)}
        update={update}
      />
    </>
  );
}
