// The mock stats store: static files under a base URL (here the repo's data/
// directory served by Vite), fetched once per patch and cached in the browser.
// The real store will serve the same files.

export type StatsIndex = { latest: string; mock?: boolean };

const CACHE_PREFIX = "draftgpt.stats.";

/** The latest patch's stats file as text, from the cache when possible. */
export async function loadStats(base = ""): Promise<{ patch: string; text: string }> {
  const index: StatsIndex = await getJson(`${base}/stats/index.json`);
  const key = CACHE_PREFIX + index.latest;
  const cached = readCache(key);
  if (cached) return { patch: index.latest, text: cached };
  const res = await fetch(`${base}/stats/${index.latest}.json`);
  if (!res.ok) throw new Error(`stats ${index.latest}: HTTP ${res.status}`);
  const text = await res.text();
  writeCache(key, text);
  return { patch: index.latest, text };
}

async function getJson<T>(url: string): Promise<T> {
  // The index must be fresh: it says which patch is current.
  const res = await fetch(url, { cache: "no-cache" });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return (await res.json()) as T;
}

// Storage can be unavailable (private windows, blocked site data); the app
// then just fetches every time.
function readCache(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeCache(key: string, value: string) {
  try {
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith(CACHE_PREFIX) && k !== key) localStorage.removeItem(k);
    }
    localStorage.setItem(key, value);
  } catch {
    /* not cached */
  }
}
