import { useEffect, useState } from "react";
import { Modal, Tabs, message, Alert, Input } from "antd";
import { Copy, ShieldCheck, AlertTriangle, Lock } from "lucide-react";
import QRCode from "qrcode";
import {
  KIND_LABELS,
  stringifyEnvelope,
  stringifyEncrypted,
  type Envelope,
} from "@/lib/configShare";

/**
 * 配置导出弹窗。
 *
 * 三个段：
 *   - 顶部：加密 PIN 输入（默认填 6 位随机数字，用户可清空表示明文）
 *   - JSON 文本：可复制
 *   - QR 码：手机端扫一下即可导入
 *
 * 当 PIN 非空 → envelope 走 v1-enc 加密层（PBKDF2 + AES-GCM-256）
 * 接收方扫码后必须输入相同 PIN 才能解密。
 */
export function ShareConfigModal({
  open,
  onClose,
  envelope,
}: {
  open: boolean;
  onClose: () => void;
  envelope: Envelope | null;
}) {
  // 默认填 6 位随机数字 PIN，鼓励用户用加密
  const [pin, setPin] = useState("");
  const [text, setText] = useState("");
  const [qrDataUrl, setQrDataUrl] = useState("");
  const [genErr, setGenErr] = useState<string | null>(null);

  // 打开时给个 6 位随机 PIN（用户改/清空都可以）
  useEffect(() => {
    if (open) {
      const rand = String(Math.floor(100000 + Math.random() * 900000));
      setPin(rand);
    }
  }, [open]);

  // envelope / pin 变化 → 重生成 JSON + QR
  useEffect(() => {
    if (!envelope) {
      setText("");
      setQrDataUrl("");
      return;
    }
    let alive = true;
    void (async () => {
      try {
        let pretty: string;
        let compact: string;
        if (pin.trim()) {
          // 加密
          compact = await stringifyEncrypted(envelope, pin.trim(), false);
          pretty = JSON.stringify(JSON.parse(compact), null, 2);
        } else {
          // 明文
          pretty = stringifyEnvelope(envelope, true);
          compact = stringifyEnvelope(envelope, false);
        }
        if (!alive) return;
        setText(pretty);
        setGenErr(null);
        const url = await QRCode.toDataURL(compact, {
          errorCorrectionLevel: "M",
          width: 320,
          margin: 1,
        });
        if (alive) setQrDataUrl(url);
      } catch (e) {
        if (alive) {
          setGenErr(`生成失败：${(e as Error).message}`);
          setQrDataUrl("");
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [envelope, pin]);

  async function copyJson() {
    try {
      await navigator.clipboard.writeText(text);
      message.success("已复制到剪贴板");
    } catch {
      message.error("自动复制失败，请手动选中复制");
    }
  }

  const isEncrypted = pin.trim().length > 0;
  const title = envelope
    ? `分享 ${KIND_LABELS[envelope.kind]}`
    : "分享配置";

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title={title}
      footer={null}
      destroyOnClose
      width={440}
    >
      {/* PIN 输入 */}
      <div className="mb-3 rounded-lg border border-slate-200 bg-slate-50 p-3">
        <div className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-slate-700">
          <Lock size={12} />
          加密 PIN（接收方需要相同 PIN 才能导入）
        </div>
        <Input
          value={pin}
          onChange={(e) => setPin(e.target.value)}
          placeholder="留空 = 明文导出（不推荐）"
          maxLength={32}
          autoComplete="off"
        />
        {isEncrypted ? (
          <div className="mt-1.5 flex items-center gap-1 text-[11px] text-green-600">
            <ShieldCheck size={11} /> 已加密（AES-GCM-256 + PBKDF2 100k）
          </div>
        ) : (
          <div className="mt-1.5 flex items-center gap-1 text-[11px] text-amber-600">
            <AlertTriangle size={11} /> 未加密 — API Key / 密码会明文出现在 QR /
            JSON 中
          </div>
        )}
      </div>

      {genErr && (
        <Alert
          type="error"
          showIcon
          className="!mb-3"
          message={genErr}
        />
      )}

      <Tabs
        items={[
          {
            key: "json",
            label: "JSON 文本",
            children: (
              <div>
                <textarea
                  readOnly
                  value={text}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                  className="w-full h-40 rounded-lg border border-slate-200 bg-slate-50 p-3 font-mono text-xs leading-relaxed"
                />
                <button
                  onClick={copyJson}
                  className="mt-3 flex w-full items-center justify-center gap-1.5 rounded-lg bg-[#1677FF] py-2 text-sm font-medium text-white active:scale-95 transition-transform"
                >
                  <Copy size={14} /> 复制 JSON
                </button>
              </div>
            ),
          },
          {
            key: "qr",
            label: "扫码导入",
            children: (
              <div className="flex flex-col items-center gap-2">
                {qrDataUrl ? (
                  <img
                    src={qrDataUrl}
                    alt="config QR code"
                    className="h-72 w-72 rounded-lg border border-slate-200 bg-white p-2"
                  />
                ) : (
                  <div className="h-72 w-72 flex items-center justify-center rounded-lg border border-dashed border-slate-300 text-sm text-slate-400">
                    QR 生成中…
                  </div>
                )}
                {isEncrypted && (
                  <div className="rounded bg-green-50 px-2 py-1 text-[11px] text-green-700">
                    🔒 PIN：<strong>{pin}</strong>{" "}
                    <span className="text-slate-500">（口头告诉接收方）</span>
                  </div>
                )}
                <p className="text-center text-xs text-slate-500">
                  在另一台设备的 <strong>导入配置</strong> → <strong>扫码</strong>
                </p>
              </div>
            ),
          },
        ]}
      />
    </Modal>
  );
}
