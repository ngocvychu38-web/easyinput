import { describe, expect, it } from "vitest";
import { DEFAULT_DOUBAO_CONFIG, DEFAULT_SETTINGS } from "./types";

describe("EasyInput Intel Mac defaults", () => {
  it("uses Intel Mac product defaults", () => {
    expect(DEFAULT_SETTINGS.inputHotkey).toBe("RightCommand");
    expect(DEFAULT_SETTINGS.editHotkey).toBe("RightOption");
    expect(DEFAULT_SETTINGS.triggerMode).toBe("Hold");
    expect(DEFAULT_SETTINGS.inputMode).toBe("Auto");
  });
  it("keeps overlay opacity within the safe range", () => {
    expect(DEFAULT_SETTINGS.overlayOpacity).toBeGreaterThanOrEqual(0);
    expect(DEFAULT_SETTINGS.overlayOpacity).toBeLessThanOrEqual(1);
  });
  it("uses the official Doubao streaming endpoint and 2.0 resource", () => {
    expect(DEFAULT_DOUBAO_CONFIG.endpoint).toBe("wss://openspeech.bytedance.com/api/v3/sauc/bigmodel");
    expect(DEFAULT_DOUBAO_CONFIG.resourceId).toBe("volc.bigasr.sauc.duration");
  });
});
