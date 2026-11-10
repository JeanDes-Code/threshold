import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { createApi, createTurnClient, type Invoke } from "./api";
import { createPlaySession } from "./play/session";

const tauriInvoke: Invoke = (cmd, args) => invoke(cmd, args);

export const api = createApi(tauriInvoke);
export const turns = createTurnClient(tauriInvoke);
export const play = createPlaySession(api);

/** Calls `handler` for each `name` event while the component is mounted. */
export function useEvent<T>(name: string, handler: (payload: T) => void): void {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void listen<T>(name, (event) => latest.current(event.payload)).then((unlisten) => {
      if (gone) unlisten();
      else off = unlisten;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [name]);
}
