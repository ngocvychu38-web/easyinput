import { describe, expect, it, vi } from "vitest";
import { formatOperationError, OperationTimeoutError, withTimeout } from "./async";

describe("async operation guard", () => {
  it("returns a completed operation", async () => {
    await expect(withTimeout(Promise.resolve("ok"), 100, "超时")).resolves.toBe("ok");
  });
  it("rejects a stalled operation and clears the busy state path", async () => {
    vi.useFakeTimers();
    const result = withTimeout(new Promise<string>(() => undefined), 15_000, "连接超时");
    const assertion = expect(result).rejects.toBeInstanceOf(OperationTimeoutError);
    await vi.advanceTimersByTimeAsync(15_000);
    await assertion;
    vi.useRealTimers();
  });
  it("formats Tauri command rejections", () => {
    expect(formatOperationError(new Error("invoke failed"))).toBe("invoke failed");
    expect(formatOperationError("keychain denied")).toBe("keychain denied");
  });
});
