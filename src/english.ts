import type { RealtimeCallState } from './types';

export const ENGLISH_PROTOCOL = 'easyinput:learning:v1';
export type LearningAction =
  | { kind: 'context'; instructions: string }
  | { kind: 'text' | 'speak'; text: string }
  | { kind: 'mute'; muted: boolean }
  | { kind: 'sound'; enabled: boolean }
  | { kind: 'interrupt' | 'stop' };
export interface LearningRequest {
  type: typeof ENGLISH_PROTOCOL; clientId: string; requestId: string; token: string;
  ownerId: string; action: LearningAction | { kind: 'start'; instructions: string; greeting: string };
}
export interface LearningTranscript { sessionId: string; id: string; role: 'user' | 'assistant'; text: string }
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const bounded = (value: unknown, max: number, empty = false) => typeof value === 'string' && (empty || !!value.trim()) && new TextEncoder().encode(value).length <= max;
export function isLearningHello(value: unknown): value is { type: string; kind: 'hello'; clientId: string } {
  return record(value) && value.type === ENGLISH_PROTOCOL && value.kind === 'hello' && typeof value.clientId === 'string' && uuid.test(value.clientId);
}
export function isLearningRequest(value: unknown): value is LearningRequest {
  if (!record(value) || value.type !== ENGLISH_PROTOCOL || !['clientId', 'requestId', 'token', 'ownerId'].every(k => typeof value[k] === 'string' && uuid.test(value[k] as string)) || !record(value.action)) return false;
  const a = value.action;
  switch (a.kind) {
    case 'start': return bounded(a.instructions, 16000) && bounded(a.greeting, 4000, true);
    case 'context': return bounded(a.instructions, 16000);
    case 'text': case 'speak': return bounded(a.text, 4000);
    case 'mute': return typeof a.muted === 'boolean';
    case 'sound': return typeof a.enabled === 'boolean';
    case 'stop': case 'interrupt': return true;
    default: return false;
  }
}
export function resolveEnglishUrl(raw?: string): string {
  const url = new URL(raw || 'http://localhost:3000/');
  if (url.username || url.password || !(url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1'].includes(url.hostname)))) throw new Error('英语学习地址须为 HTTPS 或本机服务');
  return url.href;
}
export function learningIsActive(call: RealtimeCallState) { return ['Connecting', 'Listening', 'Speaking', 'Closing'].includes(call.phase); }
