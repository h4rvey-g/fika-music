export class AllPromisesRejectedError extends Error {
  readonly errors: unknown[];

  constructor(errors: unknown[]) {
    super("All playback candidates failed.");
    this.name = "AllPromisesRejectedError";
    this.errors = errors;
  }
}

export async function firstSuccessfulWithTimeout<T>(
  promises: Promise<T>[],
  timeoutMs: number,
  signal?: AbortSignal,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;

  const timeout = new Promise<never>((_, reject) => {
    onAbort = () => reject(abortError());
    if (signal?.aborted) {
      onAbort();
      return;
    }
    signal?.addEventListener("abort", onAbort, { once: true });
    timer = setTimeout(() => reject(new Error("Source layer timed out.")), timeoutMs);
  });

  try {
    return await Promise.race([firstSuccessful(promises), timeout]);
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
    if (onAbort) {
      signal?.removeEventListener("abort", onAbort);
    }
  }
}

function firstSuccessful<T>(promises: Promise<T>[]): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    if (!promises.length) {
      reject(new Error("No playback candidates are available."));
      return;
    }

    let failures = 0;
    const errors: unknown[] = [];
    for (const promise of promises) {
      promise.then(resolve).catch((error) => {
        errors.push(error);
        failures += 1;
        if (failures === promises.length) {
          reject(new AllPromisesRejectedError(errors));
        }
      });
    }
  });
}

function abortError() {
  return new DOMException("The operation was cancelled.", "AbortError");
}
