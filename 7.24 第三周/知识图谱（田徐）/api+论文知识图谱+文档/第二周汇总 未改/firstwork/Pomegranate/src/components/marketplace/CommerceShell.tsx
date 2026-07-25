import type { ReactNode } from "react";
import { Tag, Typography } from "antd";
import type { AiServiceDeliveryMode } from "@/types";
import "./commerce-shell.css";

const { Paragraph, Text, Title } = Typography;

export interface CommerceMetric {
  label: string;
  value: ReactNode;
  hint?: string;
  tone?: "teal" | "amber" | "coral" | "blue";
}

interface CommerceHeroProps {
  eyebrow: string;
  title: string;
  description: string;
  icon: ReactNode;
  badge?: ReactNode;
  actions?: ReactNode;
  metrics?: CommerceMetric[];
}

export function CommerceHero({
  eyebrow,
  title,
  description,
  icon,
  badge,
  actions,
  metrics = [],
}: CommerceHeroProps) {
  return (
    <section className="commerce-hero">
      <div className="commerce-hero__topline">
        <div className="commerce-hero__identity">
          <div className="commerce-hero__mark">{icon}</div>
          <div>
            <div className="commerce-hero__eyebrow">{eyebrow}</div>
            <div className="commerce-hero__title-line">
              <Title level={2}>{title}</Title>
              {badge}
            </div>
            <Paragraph>{description}</Paragraph>
          </div>
        </div>
        {actions && <div className="commerce-hero__actions">{actions}</div>}
      </div>
      {metrics.length > 0 && (
        <div className="commerce-metrics">
          {metrics.map((metric) => (
            <div
              key={metric.label}
              className={`commerce-metric commerce-metric--${metric.tone ?? "teal"}`}
            >
              <Text className="commerce-metric__label">{metric.label}</Text>
              <div className="commerce-metric__value">{metric.value}</div>
              {metric.hint && <Text className="commerce-metric__hint">{metric.hint}</Text>}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

const STATUS_LABELS: Record<string, string> = {
  draft: "草稿",
  submitted: "待审核",
  under_review: "审核中",
  approved: "已批准",
  published: "已发布",
  pending_review: "等待审核",
  active: "上架中",
  rejected: "已驳回",
  suspended: "已暂停",
  revoked: "已吊销",
  delisted: "已下架",
  passed: "检查通过",
  warning: "需要复核",
  failed: "检查失败",
  not_scanned: "尚未扫描",
};

const STATUS_COLORS: Record<string, string> = {
  draft: "default",
  submitted: "gold",
  under_review: "processing",
  approved: "success",
  published: "success",
  pending_review: "gold",
  active: "success",
  rejected: "error",
  suspended: "warning",
  revoked: "error",
  delisted: "volcano",
  passed: "success",
  warning: "warning",
  failed: "error",
  not_scanned: "default",
};

export function statusLabel(status: string) {
  return STATUS_LABELS[status] ?? status;
}

export function CommerceStatusTag({ status }: { status: string }) {
  return <Tag color={STATUS_COLORS[status] ?? "default"}>{statusLabel(status)}</Tag>;
}

const DELIVERY_LABELS: Record<AiServiceDeliveryMode, string> = {
  byok: "用户自备凭据",
  "hosted-api": "开发者托管 API",
  "remote-mcp": "远程 MCP",
};

export function deliveryModeLabel(mode?: AiServiceDeliveryMode | null) {
  return mode ? DELIVERY_LABELS[mode] : "本地能力包";
}

export function DeliveryModeTag({ mode }: { mode?: AiServiceDeliveryMode | null }) {
  if (!mode) return <Tag>本地能力包</Tag>;
  const color = mode === "byok" ? "cyan" : mode === "hosted-api" ? "orange" : "geekblue";
  return <Tag color={color}>{DELIVERY_LABELS[mode]}</Tag>;
}
