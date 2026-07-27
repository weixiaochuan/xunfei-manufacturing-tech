import { useEffect, useMemo, useState } from "react";
import { Button, Input, Modal, Space, Typography } from "antd";

const { Text } = Typography;

const MINUTES_PER_HOUR = 60;
const MIN_TOTAL_MINUTES = 30;
const MAX_TOTAL_MINUTES = 24 * MINUTES_PER_HOUR;
const STEP_MINUTES = 30;

const HOUR_OPTIONS = Array.from({ length: 25 }, (_, value) => value);
const MINUTE_OPTIONS = [0, 30];

export interface DailyTimeWheelPickerProps {
  value?: number | string | null;
  disabled?: boolean;
  onChange?: (value: number) => void;
}

export function parseDailyTime(value?: number | string | null): number {
  if (typeof value === "number" && Number.isFinite(value)) {
    return clampTotalMinutes(Math.round(value * MINUTES_PER_HOUR));
  }

  if (typeof value !== "string") {
    return MINUTES_PER_HOUR;
  }

  const normalized = value.trim();
  if (!normalized) {
    return MINUTES_PER_HOUR;
  }

  const numeric = Number(normalized);
  if (Number.isFinite(numeric)) {
    return clampTotalMinutes(Math.round(numeric * MINUTES_PER_HOUR));
  }

  const hourMatch = normalized.match(/(\d+(?:\.\d+)?)\s*(?:小时|h|hour)/i);
  const minuteMatch = normalized.match(/(\d+)\s*(?:分钟|min|minute)/i);
  const hours = hourMatch ? Number(hourMatch[1]) : 0;
  const minutes = minuteMatch ? Number(minuteMatch[1]) : 0;

  if (!Number.isFinite(hours) || !Number.isFinite(minutes)) {
    return MINUTES_PER_HOUR;
  }

  const totalMinutes = Math.round(hours * MINUTES_PER_HOUR + minutes);
  return clampTotalMinutes(totalMinutes || MINUTES_PER_HOUR);
}

export function formatDailyTime(totalMinutes: number): string {
  const safeMinutes = clampTotalMinutes(totalMinutes);
  const hours = Math.floor(safeMinutes / MINUTES_PER_HOUR);
  const minutes = safeMinutes % MINUTES_PER_HOUR;

  if (hours <= 0) {
    return `每天 ${minutes} 分钟`;
  }

  if (minutes === 0) {
    return `每天 ${hours} 小时`;
  }

  return `每天 ${hours} 小时 ${minutes} 分钟`;
}

export default function DailyTimeWheelPicker({
  value,
  disabled,
  onChange,
}: DailyTimeWheelPickerProps) {
  const currentMinutes = useMemo(() => parseDailyTime(value), [value]);
  const [open, setOpen] = useState(false);
  const [draftMinutes, setDraftMinutes] = useState(currentMinutes);

  useEffect(() => {
    if (!open) {
      setDraftMinutes(currentMinutes);
    }
  }, [currentMinutes, open]);

  const selectedHours = Math.floor(draftMinutes / MINUTES_PER_HOUR);
  const selectedMinutes = draftMinutes % MINUTES_PER_HOUR;

  function updateDraft(nextHours: number, nextMinutes: number) {
    const minutes = clampTotalMinutes(nextHours * MINUTES_PER_HOUR + nextMinutes);
    setDraftMinutes(snapToStep(minutes));
  }

  function handleConfirm() {
    const safeMinutes = snapToStep(draftMinutes);
    onChange?.(safeMinutes / MINUTES_PER_HOUR);
    setOpen(false);
  }

  return (
    <>
      <Input
        readOnly
        disabled={disabled}
        value={formatDailyTime(currentMinutes)}
        placeholder="请选择每日可投入时间"
        onClick={() => {
          if (!disabled) {
            setDraftMinutes(currentMinutes);
            setOpen(true);
          }
        }}
      />
      <Modal
        title="选择每日学习时间"
        open={open}
        onCancel={() => setOpen(false)}
        onOk={handleConfirm}
        okText="确定"
        cancelText="取消"
        destroyOnHidden
      >
        <div className="space-y-4">
          <Text type="secondary">
            用半小时为步长规划每日投入时间，后续诊断和 fallback 计划会据此估算任务强度。
          </Text>
          <div className="grid grid-cols-2 gap-4">
            <WheelColumn
              label="小时"
              options={HOUR_OPTIONS}
              value={selectedHours}
              onChange={(nextHours) => updateDraft(nextHours, selectedMinutes)}
            />
            <WheelColumn
              label="分钟"
              options={MINUTE_OPTIONS}
              value={selectedMinutes}
              onChange={(nextMinutes) => updateDraft(selectedHours, nextMinutes)}
            />
          </div>
          <div className="rounded-lg border border-orange-100 bg-orange-50 px-4 py-3">
            <Space direction="vertical" size={2}>
              <Text type="secondary">当前选择</Text>
              <Text strong>{formatDailyTime(draftMinutes)}</Text>
            </Space>
          </div>
        </div>
      </Modal>
    </>
  );
}

interface WheelColumnProps {
  label: string;
  options: number[];
  value: number;
  onChange: (value: number) => void;
}

function WheelColumn({ label, options, value, onChange }: WheelColumnProps) {
  return (
    <div className="rounded-xl border border-neutral-200 bg-white p-3">
      <div className="mb-3 text-center text-sm font-medium text-neutral-500">{label}</div>
      <div className="grid max-h-56 gap-2 overflow-y-auto pr-1">
        {options.map((option) => {
          const active = option === value;
          return (
            <Button
              key={option}
              type={active ? "primary" : "default"}
              block
              onClick={() => onChange(option)}
            >
              {option.toString().padStart(2, "0")}
            </Button>
          );
        })}
      </div>
    </div>
  );
}

function snapToStep(minutes: number): number {
  const snapped = Math.round(minutes / STEP_MINUTES) * STEP_MINUTES;
  return clampTotalMinutes(snapped);
}

function clampTotalMinutes(minutes: number): number {
  if (!Number.isFinite(minutes)) {
    return MINUTES_PER_HOUR;
  }
  return Math.min(MAX_TOTAL_MINUTES, Math.max(MIN_TOTAL_MINUTES, Math.round(minutes)));
}
