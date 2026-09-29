export const LOCAL_COCKPIT_URL = "http://localhost:3001/dashboard";

export function resolveCockpitUrl(configuredUrl: string | undefined, _development: boolean) {
  const candidate = configuredUrl?.trim() || LOCAL_COCKPIT_URL;
  try {
    const url = new URL(candidate);
    if (url.protocol !== "http:" || !["localhost", "127.0.0.1"].includes(url.hostname) || url.username || url.password) return LOCAL_COCKPIT_URL;
    return url.toString();
  } catch {
    return LOCAL_COCKPIT_URL;
  }
}

export function cockpitOrigin(url: string) {
  return new URL(url).origin;
}

export type CockpitCommand = {
  action: "navigate" | "activateSystem" | "getState";
  target?: string;
  parameters?: { cohortId?: number };
};

export type CockpitCommandMessage = {
  type: "easyinput:cockpit-command";
  requestId: string;
  command: CockpitCommand;
};

export type CockpitCommandRequest = {
  requestId: string;
  responseEvent: string;
  command: CockpitCommand;
};

export type CockpitResultMessage = {
  type: "easyinput:cockpit-result";
  requestId: string;
  result: unknown;
};

export function isCockpitResultMessage(value: unknown): value is CockpitResultMessage {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<CockpitResultMessage>;
  return candidate.type === "easyinput:cockpit-result" && typeof candidate.requestId === "string" && "result" in candidate;
}
