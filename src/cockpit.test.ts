import { describe, expect, it } from "vitest";
import { cockpitOrigin, isCockpitResultMessage, LOCAL_COCKPIT_URL, resolveCockpitUrl } from "./cockpit";

describe("cockpit URL policy", () => {
  it("uses the local cockpit during development", () => {
    expect(resolveCockpitUrl(undefined, true)).toBe(LOCAL_COCKPIT_URL);
  });

  it("uses the local cockpit for production builds", () => {
    expect(resolveCockpitUrl(undefined, false)).toBe(LOCAL_COCKPIT_URL);
  });

  it("accepts only explicit local URLs", () => {
    expect(resolveCockpitUrl("http://127.0.0.1:3001/dashboard", true)).toBe("http://127.0.0.1:3001/dashboard");
    expect(resolveCockpitUrl("https://cockpit.example.com/dashboard", true)).toBe(LOCAL_COCKPIT_URL);
    expect(resolveCockpitUrl("javascript:alert(1)", true)).toBe(LOCAL_COCKPIT_URL);
    expect(resolveCockpitUrl("not a url", false)).toBe(LOCAL_COCKPIT_URL);
  });

  it("derives the exact postMessage target origin", () => {
    expect(cockpitOrigin("http://localhost:3000/dashboard/cohorts")).toBe("http://localhost:3000");
  });

  it("accepts only structured cockpit result messages", () => {
    expect(isCockpitResultMessage({ type: "easyinput:cockpit-result", requestId: "request-1", result: { ok: true } })).toBe(true);
    expect(isCockpitResultMessage({ type: "easyinput:cockpit-result", result: { ok: true } })).toBe(false);
    expect(isCockpitResultMessage({ type: "other", requestId: "request-1", result: {} })).toBe(false);
  });
});
