import { forwardRef, useEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import { Alert, Input, Modal } from "antd";

const HOURS = Array.from({ length: 25 }, (_, index) => index);
const MINUTES = [0, 30];
const ITEM_HEIGHT = 44;
const VISIBLE_ITEMS = 5;
const WHEEL_HEIGHT = ITEM_HEIGHT * VISIBLE_ITEMS;
const MIN_TOTAL_MINUTES = 30;
const MAX_TOTAL_MINUTES = 24 * 60;

export interface DailyTimeWheelPickerProps {
  value?: string | number | null;
  onChange?: (value: number) => void;
}

export function formatDailyTime(totalMinutes: number): string {
  const safeMinutes = clampTotalMinutes(totalMinutes);
  const hours = Math.floor(safeMinutes / 60);
  const minutes = safeMinutes % 60;

  if (hours === 0) {
    return `每天${minutes}分钟`;
  }
  if (minutes === 0) {
    return `每天${hours}小时`;
  }
  return `每天${hours}小时${minutes}分钟`;
}

export function parseDailyTime(value?: string | number | null): number {
  if (value == null || value === "") return 60;

  if (typeof value === "number") {
    const totalMinutes = value <= 24 ? value * 60 : value;
    return clampTotalMinutes(totalMinutes);
  }

  const normalized = String(value).replace(/\s/g, "");
  const decimalHour = normalized.match(/每天?(\d+(?:\.\d+)?)小时/);
  const hourMatch = normalized.match(/(\d+)小时/);
  const minuteMatch = normalized.match(/(\d+)分钟/);

  if (decimalHour?.[1]?.includes(".")) {
    return clampTotalMinutes(Math.round(Number(decimalHour[1]) * 60));
  }

  if (!hourMatch && !minuteMatch) {
    const numericParts = Array.from(normalized.matchAll(/\d+(?:\.\d+)?/g)).map((match) =>
      Number(match[0]),
    );
    if (numericParts.length >= 2) {
      return clampTotalMinutes(numericParts[0] * 60 + numericParts[1]);
    }
    if (numericParts.length === 1) {
      const first = numericParts[0];
      return clampTotalMinutes(first <= 24 ? first * 60 : first);
    }
  }

  const hours = hourMatch ? Number(hourMatch[1]) : 0;
  const minutes = minuteMatch ? Number(minuteMatch[1]) : 0;
  const total = hours * 60 + minutes;
  return clampTotalMinutes(total || 60);
}

export default function DailyTimeWheelPicker({
  value,
  onChange,
}: DailyTimeWheelPickerProps) {
  const [open, setOpen] = useState(false);
  const [draftMinutes, setDraftMinutes] = useState(parseDailyTime(value));
  const hourRef = useRef<HTMLDivElement | null>(null);
  const minuteRef = useRef<HTMLDivElement | null>(null);
  const hourScrollTimer = useRef<number | null>(null);
  const minuteScrollTimer = useRef<number | null>(null);

  const selectedHour = Math.floor(draftMinutes / 60);
  const selectedMinute = draftMinutes % 60;
  const displayValue =
    value == null || value === "" ? formatDailyTime(draftMinutes) : formatDailyTime(parseDailyTime(value));
  const showLongTimeHint = draftMinutes > 12 * 60;

  useEffect(() => {
    if (!open) return;
    const nextMinutes = parseDailyTime(value);
    setDraftMinutes(nextMinutes);
    window.setTimeout(() => {
      scrollToValue(hourRef.current, Math.floor(nextMinutes / 60), HOURS);
      scrollToValue(minuteRef.current, nextMinutes % 60, MINUTES);
    }, 0);
  }, [open, value]);

  const hourItems = useMemo(
    () =>
      HOURS.map((hour) => ({
        value: hour,
        label: `${hour} 小时`,
      })),
    [],
  );
  const minuteItems = useMemo(
    () =>
      MINUTES.map((minute) => ({
        value: minute,
        label: `${String(minute).padStart(2, "0")} 分钟`,
      })),
    [],
  );

  function updateDraft(hour: number, minute: number) {
    setDraftMinutes(normalizeHourMinute(hour, minute));
  }

  function handleOk() {
    onChange?.(draftMinutes / 60);
    setOpen(false);
  }

  function handleCancel() {
    setDraftMinutes(parseDailyTime(value));
    setOpen(false);
  }

  function handleHourScroll() {
    scheduleSnap(hourScrollTimer, () => {
      const hour = readSnappedValue(hourRef.current, HOURS);
      updateDraft(hour, selectedMinute);
      scrollToValue(hourRef.current, hour, HOURS);
      const normalized = normalizeHourMinute(hour, selectedMinute);
      scrollToValue(minuteRef.current, normalized % 60, MINUTES);
    });
  }

  function handleMinuteScroll() {
    scheduleSnap(minuteScrollTimer, () => {
      const minute = readSnappedValue(minuteRef.current, MINUTES);
      updateDraft(selectedHour, minute);
      const normalized = normalizeHourMinute(selectedHour, minute);
      scrollToValue(hourRef.current, Math.floor(normalized / 60), HOURS);
      scrollToValue(minuteRef.current, normalized % 60, MINUTES);
    });
  }

  return (
    <>
      <Input
        readOnly
        value={displayValue}
        placeholder="请选择每日可投入时间"
        onClick={() => setOpen(true)}
      />
      <Modal
        title="每日学习时间"
        open={open}
        okText="确定"
        cancelText="取消"
        width={420}
        onOk={handleOk}
        onCancel={handleCancel}
        destroyOnHidden
      >
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3 text-center">
            <div className="text-sm font-medium text-slate-600">小时</div>
            <div className="text-sm font-medium text-slate-600">分钟</div>
          </div>

          <div className="relative grid grid-cols-2 gap-3 overflow-hidden rounded-md border border-slate-200 bg-white px-3">
            <div className="pointer-events-none absolute left-3 right-3 top-1/2 h-11 -translate-y-1/2 rounded-md bg-blue-50 ring-1 ring-blue-100" />
            <WheelColumn
              ref={hourRef}
              items={hourItems}
              selectedValue={selectedHour}
              onScroll={handleHourScroll}
              onSelect={(hour) => {
                updateDraft(hour, selectedMinute);
                scrollToValue(hourRef.current, hour, HOURS);
                const normalized = normalizeHourMinute(hour, selectedMinute);
                scrollToValue(minuteRef.current, normalized % 60, MINUTES);
              }}
            />
            <WheelColumn
              ref={minuteRef}
              items={minuteItems}
              selectedValue={selectedMinute}
              onScroll={handleMinuteScroll}
              onSelect={(minute) => {
                updateDraft(selectedHour, minute);
                const normalized = normalizeHourMinute(selectedHour, minute);
                scrollToValue(hourRef.current, Math.floor(normalized / 60), HOURS);
                scrollToValue(minuteRef.current, normalized % 60, MINUTES);
              }}
            />
          </div>

          <div className="text-center text-sm font-semibold text-slate-700">
            当前选择：{formatDailyTime(draftMinutes)}
          </div>
          {showLongTimeHint ? (
            <Alert
              type="warning"
              showIcon
              message="每日学习时间较长，请根据实际可投入时间合理设置。"
            />
          ) : null}
        </div>
      </Modal>
    </>
  );
}

interface WheelColumnProps {
  items: Array<{ value: number; label: string }>;
  selectedValue: number;
  onScroll: () => void;
  onSelect: (value: number) => void;
}

const WheelColumn = forwardRef<HTMLDivElement, WheelColumnProps>(function WheelColumn(
  { items, selectedValue, onScroll, onSelect },
  ref,
) {
  const selectedIndex = items.findIndex((item) => item.value === selectedValue);

  return (
    <div
      ref={ref}
      className="relative z-10 overflow-y-auto overscroll-contain py-[88px]"
      style={{
        height: WHEEL_HEIGHT,
        scrollSnapType: "y mandatory",
        WebkitOverflowScrolling: "touch",
        scrollbarWidth: "none",
        maskImage:
          "linear-gradient(to bottom, transparent 0%, black 24%, black 76%, transparent 100%)",
      }}
      onScroll={onScroll}
    >
      {items.map((item, index) => {
        const selected = item.value === selectedValue;
        const distance = Math.abs(selectedIndex - index);
        const opacity = selected ? 1 : Math.max(0.32, 0.72 - distance * 0.14);

        return (
          <button
            key={item.value}
            type="button"
            className="flex w-full items-center justify-center bg-transparent text-center transition"
            style={{
              height: ITEM_HEIGHT,
              scrollSnapAlign: "center",
              border: 0,
              color: selected ? "#0f172a" : "#64748b",
              fontWeight: selected ? 700 : 500,
              opacity,
              cursor: "pointer",
            }}
            onClick={() => onSelect(item.value)}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
});

function scheduleSnap(timerRef: MutableRefObject<number | null>, callback: () => void) {
  if (timerRef.current) {
    window.clearTimeout(timerRef.current);
  }
  timerRef.current = window.setTimeout(callback, 90);
}

function readSnappedValue(element: HTMLDivElement | null, values: number[]) {
  if (!element) return values[0];
  const index = Math.round(element.scrollTop / ITEM_HEIGHT);
  return values[Math.min(Math.max(index, 0), values.length - 1)];
}

function scrollToValue(
  element: HTMLDivElement | null,
  value: number,
  values: number[],
  behavior: ScrollBehavior = "smooth",
) {
  if (!element) return;
  const index = values.indexOf(value);
  if (index < 0) return;
  element.scrollTo({ top: index * ITEM_HEIGHT, behavior });
}

function normalizeHourMinute(hour: number, minute: number) {
  if (hour <= 0 && minute <= 0) return MIN_TOTAL_MINUTES;
  if (hour >= 24 && minute > 0) return MAX_TOTAL_MINUTES;
  return clampTotalMinutes(hour * 60 + minute);
}

function clampTotalMinutes(totalMinutes: number) {
  const snapped = Math.round(totalMinutes / 30) * 30;
  return Math.min(Math.max(snapped, MIN_TOTAL_MINUTES), MAX_TOTAL_MINUTES);
}
