export function parseEmojiPrefix(name: string): { emoji: string | null; text: string; rest: string } {
  const match = name.match(/^(\p{Emoji_Presentation}|\p{Extended_Pictographic})\s*(.*)$/u);
  const text = match ? match[2] || name : name;
  return match ? { emoji: match[1], text, rest: text } : { emoji: null, text, rest: text };
}
