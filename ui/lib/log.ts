// Simple structured logger for API routes.
// All output goes to stderr (visible in docker-compose logs and terminal).

const TAG = "[ui]";

export function log(route: string, msg: string, data?: Record<string, unknown>) {
  const parts = [`${TAG} ${route}: ${msg}`];
  if (data) {
    parts.push(JSON.stringify(data));
  }
  console.error(parts.join(" "));
}

export function logError(route: string, msg: string, err?: unknown) {
  const errMsg = err instanceof Error ? err.message : String(err ?? "");
  console.error(`${TAG} ${route} ERROR: ${msg}${errMsg ? ` — ${errMsg}` : ""}`);
}
