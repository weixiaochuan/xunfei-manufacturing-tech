import type { NavigateFunction } from "react-router-dom";
import { aiChatApi } from "@/lib/api";

export async function startAiChatWithNotes(
  noteIds: number[],
  defaultTitle?: string,
  navigate?: NavigateFunction,
) {
  const conv = await aiChatApi.createConversation(defaultTitle?.trim() || undefined);
  await aiChatApi.setAttachedNotes(conv.id, noteIds);
  navigate?.(`/ai?conv=${conv.id}`);
  return conv;
}
