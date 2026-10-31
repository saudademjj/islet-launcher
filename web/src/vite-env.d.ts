/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Runs web/src/dev/selftest.ts against the real backend in dev builds. */
  readonly VITE_SELFTEST?: string;
}
