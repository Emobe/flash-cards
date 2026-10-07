// The two build-time flags the UI reads. Vite replaces them in each app's build.
interface ImportMeta {
  readonly env: {
    readonly DEV: boolean;
    /** "true" in `tauri build --debug` and `tauri dev` (the apps expose it through `envPrefix`). */
    readonly TAURI_ENV_DEBUG?: string;
  };
}
