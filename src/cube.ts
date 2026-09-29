export const CUBE_ORIGIN = "http://localhost:8787";
export const CUBE_PROTOCOL = "easyinput-cube-v1";

export function resolveCubeUrl(configuredUrl: string | undefined) {
  const value = configuredUrl?.trim() || `${CUBE_ORIGIN}/duel.html`;
  try {
    const url = new URL(value);
    if (url.protocol !== "http:" || !["localhost", "127.0.0.1"].includes(url.hostname) || url.port !== "8787" || url.username || url.password) return `${CUBE_ORIGIN}/duel.html`;
    url.pathname = "/duel.html";
    url.search = "";
    url.searchParams.set("parentOrigin", window.location.origin);
    return url.toString();
  } catch { return `${CUBE_ORIGIN}/duel.html?parentOrigin=${encodeURIComponent(window.location.origin)}`; }
}

export type CubeCommand = "scramble" | "solveBoth";
export type CubeRequest = { requestId: string; command: CubeCommand };

export function cubeCommandFromTranscript(text: string): CubeCommand | undefined {
  const compact = text.replace(/[\s“”"'‘’。，、！？；：,.!?;:]/g, "");
  if (/(一键)?(完成|复原|還原|还原).*(七步|7步)|(七步|7步).*(复原|還原|还原|完成)|一起.*(复原|還原|还原|七步|7步)|两边.*(启动|开始|完成|复原|七步)/.test(compact)) return "solveBoth";
  if (/(同步)?(重新)?(打乱|打散|乱序)(一下)?(魔方)?/.test(compact) || /(魔方).*(重新)?(打乱|打散)/.test(compact)) return "scramble";
  return undefined;
}
