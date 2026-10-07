/**
 * Whether this is a development or debug build, which gets the Developer screen (ADR 0010
 * decision 7): the dev servers (`DEV`), and Tauri's debug builds, whose Vite build is a
 * production one (`TAURI_ENV_DEBUG`, exposed by `envPrefix` in `apps/native/vite.config.ts`).
 */
export function isDeveloperBuild(): boolean {
  return import.meta.env.DEV || import.meta.env.TAURI_ENV_DEBUG === "true";
}
