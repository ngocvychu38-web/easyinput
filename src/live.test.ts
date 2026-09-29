import { describe, expect, it } from 'vitest';
import { isLiveHello, isLiveRequest, liveUrl, LIVE_PROTOCOL } from './live';
const id = 'a54c1aab-8567-4e13-a884-0bf01bbab983';
describe('live bridge boundary', () => {
  it('only offers an explicit localhost child URL and encodes the parent', () => {
    const url = new URL(liveUrl('tauri://localhost'));
    expect(url.origin).toBe('http://localhost:3002');
    expect(url.searchParams.get('parentOrigin')).toBe('tauri://localhost');
  });
  it('rejects missing identities, arbitrary IPC and oversized token writes', () => {
    const request = { type: LIVE_PROTOCOL, clientId: id, token: id, requestId: id, action: 'start' };
    expect(isLiveRequest(request)).toBe(true);
    expect(isLiveRequest({ ...request, token: '' })).toBe(false);
    expect(isLiveRequest({ ...request, action: 'execute_shell' })).toBe(false);
    expect(isLiveRequest({ ...request, action: 'saveRtcToken' })).toBe(false);
    expect(isLiveRequest({ ...request, action: 'saveRtcToken', value: 'x'.repeat(8193) })).toBe(false);
    expect(isLiveRequest({ ...request, action: 'saveRtcToken', value: '' })).toBe(true);
    expect(isLiveHello({ type: LIVE_PROTOCOL, kind: 'hello', clientId: id })).toBe(true);
    expect(isLiveHello({ type: LIVE_PROTOCOL, kind: 'hello', clientId: {} })).toBe(false);
  });
});
