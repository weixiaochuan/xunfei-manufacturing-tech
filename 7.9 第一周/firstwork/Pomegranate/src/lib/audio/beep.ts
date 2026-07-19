function createAudioContext() {
  const AudioCtor = window.AudioContext || (window as typeof window & {
    webkitAudioContext?: typeof AudioContext;
  }).webkitAudioContext;
  return AudioCtor ? new AudioCtor() : null;
}

export async function beepOnce(durationMs = 180, frequency = 880) {
  const ctx = createAudioContext();
  if (!ctx) return;

  try {
    if (ctx.state === "suspended") {
      await ctx.resume();
    }

    const oscillator = ctx.createOscillator();
    const gain = ctx.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    gain.gain.value = 0.08;
    oscillator.connect(gain);
    gain.connect(ctx.destination);

    const now = ctx.currentTime;
    const durationSec = durationMs / 1000;
    gain.gain.setValueAtTime(0.0001, now);
    gain.gain.exponentialRampToValueAtTime(0.08, now + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + durationSec);

    oscillator.start(now);
    oscillator.stop(now + durationSec);

    oscillator.onended = () => {
      void ctx.close().catch(() => {});
    };
  } catch {
    void ctx.close().catch(() => {});
  }
}

export function startBeepLoop(intervalMs = 1500) {
  void beepOnce();
  const timer = window.setInterval(() => {
    void beepOnce();
  }, intervalMs);

  return () => window.clearInterval(timer);
}
