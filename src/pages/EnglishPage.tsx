import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { BookOpen, RefreshCw } from 'lucide-react';
import { ENGLISH_PROTOCOL, isLearningHello, isLearningRequest, resolveEnglishUrl, type LearningAction, type LearningTranscript } from '../english';
import type { OperationResult, RealtimeCallState } from '../types';
import { SectionLabel } from '../components/Ui';

export function EnglishPage() {
  const frame = useRef<HTMLIFrameElement>(null);
  const [revision, setRevision] = useState(0), [status, setStatus] = useState('正在连接英语小岛…');
  const url = resolveEnglishUrl(import.meta.env.VITE_ENGLISH_URL), origin = new URL(url).origin;
  const native = '__TAURI_INTERNALS__' in window;
  useEffect(() => {
    let disposed = false, clientId = '', token = '', queue = Promise.resolve();
    let owned: { ownerId: string; sessionId: string } | undefined;
    const unlisteners: UnlistenFn[] = [];
    const seen = new Set<string>();
    const post = (data: object) => { if (!disposed && clientId) frame.current?.contentWindow?.postMessage({ type: ENGLISH_PROTOCOL, clientId, token, ...data }, origin); };
    const command = async (lease: { ownerId: string; sessionId: string }, action: LearningAction) => {
      await invoke<void>('learning_realtime_command', { ...lease, action });
      if (action.kind === 'stop') {
        const deadline = Date.now() + 6000;
        while (Date.now() < deadline) {
          const state = await invoke<RealtimeCallState>('get_realtime_call_state');
          if (state.sessionId !== lease.sessionId || state.phase === 'Idle' || state.phase === 'Error') return;
          await new Promise(resolve => window.setTimeout(resolve, 100));
        }
        throw new Error('语音仍在结束中，请稍后重试');
      }
    };
    const release = async () => { const lease = owned; owned = undefined; if (lease) await command(lease, { kind: 'stop' }).catch(() => {}); };
    const receive = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || event.origin !== origin || disposed) return;
      const data: unknown = event.data;
      if (isLearningHello(data)) {
        if (clientId !== data.clientId) {
          clientId = data.clientId; token = crypto.randomUUID(); seen.clear();
          queue = queue.then(release);
        }
        setStatus(native ? '实时语音由 EasyInput 提供 · 使用开发板麦克风与扬声器' : '页面预览 · 实时语音请在 EasyInput 桌面应用中使用');
        post({ kind: 'welcome', available: native }); return;
      }
      if (!isLearningRequest(data) || data.clientId !== clientId || data.token !== token || seen.has(data.requestId)) return;
      seen.add(data.requestId); if (seen.size > 1000) seen.delete(seen.values().next().value!);
      const generation = token;
      queue = queue.then(async () => {
        if (disposed || token !== generation) return;
        try {
          if (!native) throw new Error('请在 EasyInput 桌面应用中使用开发板实时语音');
          const action = data.action;
          if (action.kind === 'start') {
            if (owned) throw new Error('请先结束当前学习语音，再开启新的活动');
            const result = await invoke<OperationResult<{ sessionId: string }>>('start_realtime_call', { learning: { ownerId: data.ownerId, instructions: action.instructions, greeting: action.greeting } });
            if (!result.ok || !result.data) throw new Error(result.message || '启动失败');
            const lease = { ownerId: data.ownerId, sessionId: result.data.sessionId };
            if (disposed || token !== generation) { await command(lease, { kind: 'stop' }).catch(() => {}); return; }
            owned = lease;
            post({ kind: 'result', requestId: data.requestId, ok: true, sessionId: lease.sessionId });
            const state = await invoke<RealtimeCallState>('get_realtime_call_state');
            if (state.sessionId === lease.sessionId) post({ kind: 'state', ownerId: lease.ownerId, state });
          } else {
            if (!owned || owned.ownerId !== data.ownerId) throw new Error('该页面没有正在运行的学习会话');
            await command(owned, action);
            if (action.kind === 'stop') owned = undefined;
            if (!disposed && token === generation) post({ kind: 'result', requestId: data.requestId, ok: true });
          }
        } catch (error) {
          if (!disposed && token === generation) post({ kind: 'result', requestId: data.requestId, ok: false, message: error instanceof Error ? error.message : String(error) });
        }
      });
    };
    window.addEventListener('message', receive);
    const hardware = () => post({ kind: 'hardware' });
    window.addEventListener('easyinput:learning-hardware', hardware);
    if (native) {
      const register = <T,>(name: string, handler: (value: T) => void) => {
        void listen<T>(name, event => handler(event.payload)).then(fn => disposed ? fn() : unlisteners.push(fn)).catch(error => setStatus(String(error)));
      };
      register<RealtimeCallState>('realtime-call-state', state => {
        if (!owned || state.sessionId !== owned.sessionId) return;
        post({ kind: 'state', ownerId: owned.ownerId, state });
        if (state.phase === 'Idle' || state.phase === 'Error') owned = undefined;
      });
      register<LearningTranscript>('learning-transcript', transcript => { if (owned?.sessionId === transcript.sessionId) post({ kind: 'transcript', ownerId: owned.ownerId, transcript }); });
    }
    const timeout = window.setTimeout(() => { if (!clientId && !frame.current?.contentWindow) setStatus('英语小岛未连接。请启动英语学习应用的本地服务（3000），然后刷新。'); }, 12000);
    return () => { disposed = true; window.clearTimeout(timeout); window.removeEventListener('message', receive); window.removeEventListener('easyinput:learning-hardware', hardware); unlisteners.forEach(fn => fn()); void queue.then(release); };
  }, [origin, revision]);
  return <div className="page cockpit-page">
    <div className="cockpit-page-head"><div><SectionLabel index="07">英语学习</SectionLabel><h1><BookOpen size={22} />Milo 英语小岛</h1></div><div className="cockpit-page-actions"><button onClick={() => { setStatus('正在重新连接…'); setRevision(v => v + 1); }}><RefreshCw size={15} />刷新</button></div></div>
    <div role="status" style={{ fontSize: 13 }}>{status}</div>
    <div className="cockpit-frame-shell"><iframe key={revision} ref={frame} src={url} title="Milo 英语学习子系统" onLoad={() => setStatus(native ? '英语小岛页面已加载 · 实时语音由 EasyInput 提供' : '英语小岛页面已加载 · 实时语音请在 EasyInput 桌面应用中使用')} sandbox="allow-scripts allow-same-origin allow-forms allow-popups" /></div>
  </div>;
}
