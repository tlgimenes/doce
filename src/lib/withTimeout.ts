/**
 * Races `promise` against a timeout, rejecting if it doesn't settle in
 * time. Tauri's IPC bridge has no built-in timeout: an invoke() call that
 * never gets a response (bridge not ready yet, a dropped message, a stuck
 * backend) hangs forever with no way to observe or recover from it. This
 * gives callers a bounded wait instead.
 */
export function withTimeout<T>(promise: Promise<T>, ms: number, message = "timed out"): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}
