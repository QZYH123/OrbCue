export type ChimeSeverity = 'info' | 'attention' | 'error';

export interface ChimeSettings {
  completion: boolean;
  attention: boolean;
  failure: boolean;
}

let sharedContext: AudioContext | null = null;

function audioContextClass(): typeof AudioContext | null {
  const candidate =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  return candidate ?? null;
}

function audioContext(): AudioContext | null {
  const Ctor = audioContextClass();
  if (!Ctor) return null;
  if (!sharedContext || sharedContext.state === 'closed') {
    sharedContext = new Ctor();
  }
  return sharedContext;
}

export function shouldPlayChime(severity: ChimeSeverity, settings: ChimeSettings): boolean {
  if (severity === 'info') return settings.completion;
  if (severity === 'attention') return settings.attention;
  return settings.failure;
}

export async function unlockAudio(): Promise<void> {
  const context = audioContext();
  if (context?.state === 'suspended') {
    try {
      await context.resume();
    } catch {
      // WebView may still block until a later gesture.
    }
  }
}

export async function playChime(severity: ChimeSeverity, settings: ChimeSettings): Promise<void> {
  if (!shouldPlayChime(severity, settings)) return;
  const context = audioContext();
  if (!context) return;
  await unlockAudio();
  if (context.state !== 'running') return;
  const oscillator = context.createOscillator();
  const gain = context.createGain();
  oscillator.type = 'sine';
  oscillator.frequency.value = severity === 'error' ? 460 : severity === 'attention' ? 620 : 780;
  const now = context.currentTime;
  gain.gain.setValueAtTime(0.12, now);
  gain.gain.exponentialRampToValueAtTime(0.001, now + 0.18);
  oscillator.connect(gain).connect(context.destination);
  oscillator.start(now);
  oscillator.stop(now + 0.2);
}
