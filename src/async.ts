export class OperationTimeoutError extends Error {
  constructor(message: string) { super(message); this.name = "OperationTimeoutError"; }
}

export async function withTimeout<T>(operation: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = globalThis.setTimeout(() => reject(new OperationTimeoutError(message)), timeoutMs);
  });
  try { return await Promise.race([operation, timeout]); }
  finally { if (timer !== undefined) globalThis.clearTimeout(timer); }
}

export function formatOperationError(reason: unknown): string {
  if (reason instanceof Error && reason.message) return reason.message;
  if (typeof reason === "string" && reason.trim()) return reason;
  try { return JSON.stringify(reason); } catch { return "未知错误"; }
}
