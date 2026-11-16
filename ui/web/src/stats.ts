// The mock stats store: static files under the site's base URL (here the
// repo's data/ directory served by Vite), fetched once per version and cached
// in the browser. The real store will serve the same files.

export type StatsIndex = { latest: string; hash?: string; mock?: boolean };

const CACHE_PREFIX = "draftgpt.stats.";

/**
 * The latest patch's stats file as text. `validate` must throw for text that
 * isn't a usable stats file; only text that passes is cached, and a cached
 * copy that fails is dropped and fetched again.
 */
export async function loadStats(
  validate: (text: string) => void,
  base: string = import.meta.env.BASE_URL,
): Promise<{ patch: string; text: string }> {
  const root = base.endsWith("/") ? base : `${base}/`;
  // The index must be fresh: it says which patch and version are current.
  const index = (await getJson(`${root}stats/index.json`)) as StatsIndex;
  const key = `${CACHE_PREFIX}${index.latest}.${index.hash ?? "nohash"}`;
  const cached = readCache(key);
  if (cached != null) {
    try {
      validate(cached);
      return { patch: index.latest, text: cached };
    } catch {
      removeCache(key);
    }
  }
  const res = await fetch(`${root}stats/${encodeURIComponent(index.latest)}.json`);
  if (!res.ok) throw new Error(`stats ${index.latest}: HTTP ${res.status}`);
  if (!(res.headers.get("content-type") ?? "").includes("json")) {
    throw new Error(`stats ${index.latest}: not JSON (the file may not be deployed)`);
  }
  const text = await res.text();
  validate(text);
  writeCache(key, text);
  return { patch: index.latest, text };
}

async function getJson(url: string): Promise<unknown> {
  const res = await fetch(url, { cache: "no-cache" });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
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

function removeCache(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {
    /* nothing cached */
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
