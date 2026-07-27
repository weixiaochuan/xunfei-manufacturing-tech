import type { AutoSaveStatus } from "@/hooks/useAutoSave";

export interface DailyDraftSnapshot {
  noteId: number | null;
  date: string;
  title: string;
  content: string;
  status: AutoSaveStatus;
}

let latestSnapshot: DailyDraftSnapshot | null = null;

export function setDailyDraftSnapshot(snapshot: DailyDraftSnapshot) {
  latestSnapshot = snapshot;
}

export function getDailyDraftSnapshot(noteId: number): DailyDraftSnapshot | null {
  return latestSnapshot?.noteId === noteId ? latestSnapshot : null;
}

export function dailySnapshotHasUnpersistedContent(snapshot: DailyDraftSnapshot): boolean {
  return snapshot.status === "dirty" || snapshot.status === "saving" || snapshot.status === "error";
}
