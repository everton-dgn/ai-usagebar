/// <reference types="vite/client" />
/// <reference types="unplugin-icons/types/react" />

export {};

declare global {
  interface Window {
    ipc?: { postMessage: (msg: string) => void };
    __AIUB_APPLY__?: (raw: unknown) => void;
    __AIUB_LOCKCLICKS__?: (ms: number) => void;
    __AIUB_VISIBLE__?: (visible: boolean) => void;
    /** Open on one provider's tab (a menu-bar provider item), or `null` to open as before. */
    __AIUB_FOCUS__?: (id: string | null) => void;
  }
}
