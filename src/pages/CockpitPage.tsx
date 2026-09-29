import { emit } from "@tauri-apps/api/event";
import { LayoutDashboard, LoaderCircle, Mic, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { cockpitOrigin, isCockpitResultMessage, resolveCockpitUrl, type CockpitCommand, type CockpitCommandMessage, type CockpitCommandRequest } from "../cockpit";
import { SectionLabel } from "../components/Ui";

declare global {
  interface WindowEventMap {
    "easyinput:cockpit-command": CustomEvent<CockpitCommand>;
  }
}

export function CockpitPage({ commandRequest, onCommandComplete }: { commandRequest?: CockpitCommandRequest; onCommandComplete(requestId: string): void }) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const lastForwardedRequest = useRef("");
  const lastAnsweredRequest = useRef("");
  const [revision, setRevision] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const cockpitUrl = useMemo(
    () => resolveCockpitUrl(import.meta.env.VITE_COCKPIT_URL, import.meta.env.DEV),
    []
  );
  const targetOrigin = useMemo(() => cockpitOrigin(cockpitUrl), [cockpitUrl]);

  useEffect(() => {
    const forwardCommand = (event: WindowEventMap["easyinput:cockpit-command"]) => {
      const message: CockpitCommandMessage = {
        type: "easyinput:cockpit-command",
        requestId: crypto.randomUUID(),
        command: event.detail,
      };
      frameRef.current?.contentWindow?.postMessage(message, targetOrigin);
    };
    window.addEventListener("easyinput:cockpit-command", forwardCommand);
    return () => window.removeEventListener("easyinput:cockpit-command", forwardCommand);
  }, [targetOrigin]);

  useEffect(() => {
    if (!loaded || !commandRequest || lastForwardedRequest.current === commandRequest.requestId) return;
    const message: CockpitCommandMessage = {
      type: "easyinput:cockpit-command",
      requestId: commandRequest.requestId,
      command: commandRequest.command,
    };
    frameRef.current?.contentWindow?.postMessage(message, targetOrigin);
    lastForwardedRequest.current = commandRequest.requestId;
  }, [commandRequest, loaded, targetOrigin]);

  useEffect(() => {
    const receiveResult = (event: MessageEvent<unknown>) => {
      const message = event.data;
      if (event.source !== frameRef.current?.contentWindow || event.origin !== targetOrigin || !isCockpitResultMessage(message)) return;
      if (!commandRequest || message.requestId !== commandRequest.requestId || lastAnsweredRequest.current === message.requestId) return;
      lastAnsweredRequest.current = message.requestId;
      void emit(commandRequest.responseEvent, message.result).finally(() => onCommandComplete(message.requestId));
    };
    window.addEventListener("message", receiveResult);
    return () => window.removeEventListener("message", receiveResult);
  }, [commandRequest, onCommandComplete, targetOrigin]);

  const reload = () => {
    setLoaded(false);
    lastForwardedRequest.current = "";
    setRevision(value => value + 1);
  };

  return <div className="page cockpit-page">
    <div className="cockpit-page-head">
      <div>
        <SectionLabel index="06">运营驾驶舱</SectionLabel>
        <h1><LayoutDashboard size={22} />AI 训练营运营驾驶舱</h1>
      </div>
      <div className="cockpit-page-actions">
        <span className="cockpit-voice-hint"><Mic size={14} />开发板实时语音已接入</span>
        <button onClick={reload} aria-label="重新加载驾驶舱"><RefreshCw size={15} />刷新</button>
      </div>
    </div>
    <div className="cockpit-frame-shell">
      {!loaded && <div className="cockpit-loading"><LoaderCircle className="spin" size={20} />正在连接驾驶舱…</div>}
      <iframe
        key={revision}
        ref={frameRef}
        src={cockpitUrl}
        title="AI 训练营运营驾驶舱"
        allow="microphone"
        onLoad={() => setLoaded(true)}
      />
    </div>
  </div>;
}
