import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Box, RefreshCw } from "lucide-react";
import { CUBE_PROTOCOL, resolveCubeUrl, type CubeCommand, type CubeRequest } from "../cube";
import { getCubeModelKeys, saveCubeModelKey } from "../api";

declare global {
  interface WindowEventMap { "easyinput:cube-command": CustomEvent<CubeCommand>; }
}

export function CubePage() {
  const frame = useRef<HTMLIFrameElement>(null);
  const [revision, setRevision] = useState(0);
  const [status, setStatus] = useState("正在连接双模型魔方…");
  const url = useMemo(() => resolveCubeUrl(import.meta.env.VITE_CUBE_URL), []);
  const origin = new URL(url).origin;
  const peerClientId = useRef("");
  const token = useRef("");
  const keys = useRef<{ jev: string | null; deepseek: string | null }>({ jev: null, deepseek: null });
  const keysLoaded = useRef(false);
  const saveTimers = useRef<Partial<Record<"jev" | "deepseek", number>>>({});

  const send = useCallback((command: CubeCommand) => {
    if (!peerClientId.current || !token.current) {
      setStatus("双模型页面尚未完成握手，请稍候或刷新魔方页");
      return;
    }
    const requestId = crypto.randomUUID();
    const request: CubeRequest = { requestId, command };
    frame.current?.contentWindow?.postMessage({ type: CUBE_PROTOCOL, kind: "command", clientId: peerClientId.current, token: token.current, ...request }, origin);
    setStatus(command === "scramble" ? "已向双盘发送同步打乱指令" : "已同时向两边发送七步复原指令");
  }, [origin]);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (event.origin !== origin || event.source !== frame.current?.contentWindow || !event.data || typeof event.data !== "object") return;
      const data = event.data as Record<string, unknown>;
      if (data.type !== CUBE_PROTOCOL) return;
      if (data.kind === "hello") {
        if (typeof data.clientId !== "string" || !data.clientId) return;
        peerClientId.current = data.clientId;
        token.current = crypto.randomUUID();
        if (!keysLoaded.current) return;
        frame.current?.contentWindow?.postMessage({ type: CUBE_PROTOCOL, kind: "welcome", clientId: peerClientId.current, token: token.current, keys: keys.current }, origin);
        setStatus("双模型魔方已连接 · 说“打乱魔方”或“一起开始复原魔方”");
        return;
      }
      if (data.clientId !== peerClientId.current || data.token !== token.current) return;
      if (data.kind === "key-change" && (data.provider === "jev" || data.provider === "deepseek") && typeof data.key === "string") {
        const provider = data.provider;
        const key = data.key.slice(0, 4096);
        keys.current[provider] = key;
        window.clearTimeout(saveTimers.current[provider]);
        saveTimers.current[provider] = window.setTimeout(() => {
          void saveCubeModelKey(provider, key).then(result => {
            if (!result.ok) setStatus(result.message || "魔方密钥保存失败，请检查钥匙串授权");
            else setStatus(`${provider === "jev" ? "Jev" : "DeepSeek"} 密钥已安全保存到本机`);
          }).catch(() => setStatus("魔方密钥保存失败，请检查本机钥匙串"));
        }, 700);
        return;
      }
      if (data.kind === "status" && typeof data.message === "string") setStatus(data.message);
    };
    const command = (event: WindowEventMap["easyinput:cube-command"]) => send(event.detail);
    void getCubeModelKeys().then(saved => {
      keys.current = saved;
      keysLoaded.current = true;
      if (peerClientId.current && token.current) {
        frame.current?.contentWindow?.postMessage({ type: CUBE_PROTOCOL, kind: "welcome", clientId: peerClientId.current, token: token.current, keys: saved }, origin);
      }
    }).catch(() => {
      keysLoaded.current = true;
      if (peerClientId.current && token.current) frame.current?.contentWindow?.postMessage({ type: CUBE_PROTOCOL, kind: "welcome", clientId: peerClientId.current, token: token.current, keys: keys.current }, origin);
      setStatus("无法读取魔方本机密钥，请检查钥匙串授权");
    });
    window.addEventListener("message", receive);
    window.addEventListener("easyinput:cube-command", command);
    return () => { window.removeEventListener("message", receive); window.removeEventListener("easyinput:cube-command", command); };
  }, [origin, send]);

  const reload = () => { token.current = ""; peerClientId.current = ""; setStatus("正在重新连接…"); setRevision(value => value + 1); };
  return <section className="cube-page">
    <header className="cube-toolbar"><div><h1><Box size={22} />三阶魔方 · 双模型复原</h1><p role="status">{status}</p></div><button onClick={reload}><RefreshCw size={15} />刷新</button></header>
    <iframe key={revision} ref={frame} src={url} title="Jev 与 DeepSeek 双模型三阶魔方" sandbox="allow-scripts allow-same-origin allow-forms" onLoad={() => setStatus("页面已加载，正在连接语音控制…")} />
  </section>;
}
