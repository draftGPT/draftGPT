// Where the panel's updates come from: the Tauri app (live client or replay,
// see crates/desktop), or, in a plain browser, a demo file (`?demo=<fixture>`,
// written by `npm run demo`).
import type { DraftState, Recommendations } from "@draftgpt/draft-ui";

/** What the Rust side emits as `draft-update` (crates/desktop/src/state.rs). */
export type Update = {
  draft: DraftState | null;
  recs: Recommendations | null;
  /** Connection state in words: "League client connected", "Replaying …". */
  status: string;
  /** The window floats instead of following the client window. */
  floating: boolean;
  /** Docking works here (Windows only). */
  canDock: boolean;
};

export type Source = {
  start(onUpdate: (u: Update) => void): Promise<void>;
  setFloating?(on: boolean): Promise<void>;
};

export function inTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export function tauriSource(): Source {
  return {
    async start(onUpdate) {
      const { listen } = await import("@tauri-apps/api/event");
      const { invoke } = await import("@tauri-apps/api/core");
      await listen<Update>("draft-update", (e) => onUpdate(e.payload));
      onUpdate(await invoke<Update>("latest"));
    },
    async setFloating(on) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_floating", { on });
    },
  };
}

type DemoStep = { t: number; draft: DraftState | null; recs: Recommendations | null };

/** Plays a demo file at `speed`× its recorded pace. */
export function demoSource(name: string, speed: number): Source {
  return {
    async start(onUpdate) {
      const res = await fetch(`demo/${encodeURIComponent(name)}.json`);
      if (!res.ok) throw new Error(`demo ${name}: HTTP ${res.status}`);
      const steps = (await res.json()) as DemoStep[];
      const status = `Demo: ${name}`;
      onUpdate({ draft: null, recs: null, status, floating: true, canDock: false });
      const t0 = steps[0]?.t ?? 0;
      for (const s of steps) {
        const u = { draft: s.draft, recs: s.recs, status, floating: true, canDock: false };
        setTimeout(() => onUpdate(u), (s.t - t0) / speed);
      }
    },
  };
}
