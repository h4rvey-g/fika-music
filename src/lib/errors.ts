export function normalizeError(
  error: unknown,
  fallback = "Unexpected application error.",
): string {
  let candidate = error;
  if (typeof candidate === "string") {
    const text = candidate;
    try {
      candidate = JSON.parse(candidate) as unknown;
    } catch {
      return text;
    }
  }
  if (candidate instanceof Error) {
    return candidate.message;
  }
  if (candidate && typeof candidate === "object" && "message" in candidate) {
    const message = (candidate as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return fallback;
}

export function queryError(label: string, isError: boolean, error: unknown) {
  return isError ? `${label}: ${normalizeError(error)}` : null;
}
