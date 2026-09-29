import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isLiveHello, isLiveRequest, LIVE_ORIGIN, LIVE_PROTOCOL, liveUrl, type LiveEvent } from '../live';

declare global { interface WindowEventMap { 'easyinput:live-user-utterance': CustomEvent<{ text: string; control: boolean }>; } }

export function LivePage() {
  const frame = useRef<HTMLIFrameElement>(null);
  const [revision, setRevision] = useState(0);
  const [status, setStatus] = useState('正在连接直播工作室…');
  useEffect(() => {
    let disposed = false, clientId = '', token = '', sessionId = '';
    let queue = Promise.resolve(), unlisten: UnlistenFn | undefined;
    const owner = crypto.randomUUID();
    const pendingUserUtterances: { text: string; control: boolean }[] = [];
    const native = '__TAURI_INTERNALS__' in window;
    const seen = new Set<string>();
    const early: LiveEvent[] = [];
    let starting = false;
    const post = (data: object) => { if (!disposed && clientId) frame.current?.contentWindow?.postMessage({ type: LIVE_PROTOCOL, clientId, token, ...data }, LIVE_ORIGIN); };
    const stop = async () => {
      const id = sessionId; sessionId = '';
      if (id) await invoke('live_speech_command', { owner, sessionId: id, action: 'stop' });
    };
    const relay = (event: LiveEvent) => {
      if (event.sessionId !== sessionId) return;
      if (event.kind !== 'transcript') setStatus(event.text);
      post({ kind: 'speech', event });
      if (event.kind === 'stopped' || event.kind === 'error') sessionId = '';
    };
    const ready = native ? listen<LiveEvent>('live-speech', e => {
      if (starting && early.length < 100) early.push(e.payload); else relay(e.payload);
    }).then(fn => { if (disposed) fn(); else unlisten = fn; }) : Promise.resolve();
    const receive = (event: MessageEvent) => {
      if (disposed || event.origin !== LIVE_ORIGIN || event.source !== frame.current?.contentWindow) return;
      const data: unknown = event.data;
      if (isLiveHello(data)) {
        if (clientId !== data.clientId) {
          clientId = data.clientId; token = crypto.randomUUID(); seen.clear();
          queue = queue.then(stop).catch(error => setStatus(String(error)));
        }
        post({ kind: 'welcome', available: native });
        pendingUserUtterances.splice(0).forEach(utterance => post({ kind: 'user-utterance', ...utterance }));
        setStatus(native ? '直播工作室已连接 · 字幕和特效由开发板实时对话控制' : '界面预览 · 实时对话控制请在 EasyInput 桌面应用中使用');
        return;
      }
      if (!isLiveRequest(data) || data.clientId !== clientId || data.token !== token || seen.has(data.requestId)) return;
      seen.add(data.requestId); if (seen.size > 1000) seen.delete(seen.values().next().value!);
      const generation = token;
      queue = queue.then(async () => {
        if (disposed || generation !== token) return;
        try {
          if (!native) throw new Error('请在 EasyInput 桌面应用中使用硬件字幕和钥匙串');
          await ready;
          let value: unknown;
          switch (data.action) {
            case 'start': {
              if (sessionId) throw new Error('字幕识别已启动');
              starting = true;
              try { sessionId = await invoke<string>('start_live_speech', { owner }); }
              finally { starting = false; }
              if (disposed || generation !== token) { await stop(); early.length = 0; return; }
              value = sessionId;
              break;
            }
            case 'stop': await stop(); break;
            case 'heartbeat': if (sessionId) await invoke('live_speech_command', { owner, sessionId, action: 'heartbeat' }); break;
            case 'loadRtcToken': value = await invoke<string>('live_rtc_token', { token: null }); break;
            case 'saveRtcToken': await invoke('live_rtc_token', { token: data.value }); break;
          }
          if (generation !== token || disposed) return;
          post({ kind: 'result', requestId: data.requestId, ok: true, value });
          early.splice(0).forEach(relay);
        } catch (error) {
          early.length = 0;
          if (generation !== token || disposed) return;
          post({ kind: 'result', requestId: data.requestId, ok: false, message: String(error) });
          setStatus(String(error));
        }
      });
    };
    const userUtterance = (event: WindowEventMap['easyinput:live-user-utterance']) => {
      if (token) post({ kind: 'user-utterance', ...event.detail });
      else { pendingUserUtterances.push(event.detail); if (pendingUserUtterances.length > 16) pendingUserUtterances.shift(); }
    };
    window.addEventListener('message', receive);
    window.addEventListener('easyinput:live-user-utterance', userUtterance);
    const timeout = window.setTimeout(() => { if (!clientId) setStatus('直播服务未连接。请运行 npm run live:dev，再点击“重新连接”。'); }, 8000);
    return () => {
      disposed = true; window.clearTimeout(timeout); window.removeEventListener('message', receive); window.removeEventListener('easyinput:live-user-utterance', userUtterance); unlisten?.();
      void queue.then(stop).catch(() => {});
    };
  }, [revision]);
  return <section className="live-page">
    <div className="live-toolbar"><div><h1>直播工作室</h1><p role="status">{status}</p></div><button onClick={() => { setStatus('正在重新连接…'); setRevision(v => v + 1); }}>重新连接</button></div>
    <iframe key={revision} ref={frame} src={liveUrl(window.location.origin)} title="映色直播工作室" allow="camera; microphone; autoplay; clipboard-write" sandbox="allow-scripts allow-same-origin allow-forms" />
  </section>;
}
