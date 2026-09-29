export const LIVE_PROTOCOL = 'easyinput-live-v1';
export const LIVE_ORIGIN = 'http://localhost:3002';
export const liveUrl = (parentOrigin: string) => `${LIVE_ORIGIN}/?easyinput=1&parentOrigin=${encodeURIComponent(parentOrigin)}`;
export type LiveRequest = { type: typeof LIVE_PROTOCOL; clientId: string; token: string; requestId: string; action: 'start' | 'stop' | 'heartbeat' | 'loadRtcToken' | 'saveRtcToken'; value?: string };
export function isLiveControlCommand(text: string): boolean {
  const value = text.replace(/[\s“”"'‘’。，、！？；：,.!?;:]/g, '').replace(/[带代待]/g, '戴');
  const targets = /(?:蓝色?面具|面具|面膜|脸谱|蓝脸|胡子|胡须|猫咪|猫猫|小猫|猫耳|圣诞老人|圣诞帽|帽子和墨镜|帽子墨镜|戴帽子|戴墨镜|帽子眼镜|夏日交友|美颜|磨皮|美白|漂亮一点)/;
  const remove = /(?:取消贴纸|移除贴纸|关闭贴纸|摘掉面具|去掉胡子|恢复原样|不要贴纸|关闭特效|关掉特效)/;
  if (remove.test(value)) return true;
  if (value.length <= 16 && targets.test(value)) return true;
  return /(?:戴|加|变成|換成|换成|打开|用|来点|来个|整一个|给我|调|调到|设置为).{0,8}(?:蓝色?面具|面具|面膜|脸谱|蓝脸|胡子|胡须|猫咪|猫猫|小猫|猫耳|圣诞老人|圣诞帽|帽子和墨镜|帽子墨镜|戴帽子|戴墨镜|帽子眼镜|夏日交友|美颜|磨皮|美白|漂亮一点)/.test(value);
}
const id = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f-]{36}$/i.test(value);
export function isLiveHello(data: unknown): data is { type: string; kind: 'hello'; clientId: string; capabilities: Record<string, boolean> } {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return d.type === LIVE_PROTOCOL && d.kind === 'hello' && id(d.clientId);
}
export function isLiveRequest(data: unknown): data is LiveRequest {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return d.type === LIVE_PROTOCOL && id(d.clientId) && id(d.token) && id(d.requestId)
    && ['start', 'stop', 'heartbeat', 'loadRtcToken', 'saveRtcToken'].includes(String(d.action))
    && (d.action !== 'saveRtcToken' || (typeof d.value === 'string' && d.value.length <= 8192));
}
export type LiveEvent = { sessionId: string; kind: 'state' | 'transcript' | 'stopped' | 'error'; text: string; definite: boolean; utteranceId: string };
