/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** The smhub-ui package version this build belongs to (set by packaging/build-ipk.sh; absent in development). */
  readonly VITE_UI_VERSION?: string;
  /** "1" puts the UPS board page in a release build (it is always in development builds). */
  readonly VITE_UPS?: string;
}
