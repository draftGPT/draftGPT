// devserver page. Everything from the server goes through textContent, never
// innerHTML, so event data can't inject markup.
"use strict";

// The session is served under two plugins; same shape.
const SESSION_URIS = ["/lol-champ-select/v1/session", "/lol-lobby-team-builder/champ-select/v1/session"];
// Champions offered in subset-pick modes; the session doesn't list them.
const SUBSET_URI = "/lol-lobby-team-builder/champ-select/v1/subset-champion-list";
// Carries dodgeData, which says why a champ select ended early.
const MATCHMAKING_URI = "/lol-lobby-team-builder/v1/matchmaking";
const MAX_LOG = 500;

const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

// Champion names from Data Dragon; falls back to ids if offline.
let champNames = {};
(async () => {
  try {
    const versions = await (await fetch("https://ddragon.leagueoflegends.com/api/versions.json")).json();
    const data = await (await fetch(`https://ddragon.leagueoflegends.com/cdn/${versions[0]}/data/en_US/champion.json`)).json();
    for (const c of Object.values(data.data)) champNames[Number(c.key)] = c.name;
    if (lastSession) renderSession(lastSession);
  } catch (e) {
    console.warn("Data Dragon unavailable, showing champion ids", e);
  }
})();
const champ = (id) => (id ? champNames[id] || `#${id}` : null);

// ---- status ----------------------------------------------------------------

function renderStatus(s) {
  const c = s.connection;
  const dot = $("conn-dot");
  if (c.state === "connected") {
    dot.className = "dot ok";
    const v = c.client.version ? ` · client ${c.client.version}` : "";
    $("conn").textContent = `League client connected (port ${c.client.port})${v}`;
  } else if (c.state === "disconnected") {
    dot.className = "dot err";
    $("conn").textContent = `League client: ${c.reason}`;
  } else {
    dot.className = "dot";
    $("conn").textContent = "waiting for League client…";
  }
  $("phase").textContent = s.phase;
  $("auto").checked = s.auto;
  const r = s.recording;
  $("rec-dot").className = r ? "dot rec" : "dot";
  $("rec").textContent = r
    ? `recording ${r.name} · ${r.events} events · ${r.wire_frames} wire frames${r.by_auto ? " (auto)" : ""}`
    : "not recording";
  $("start").disabled = !!r;
  $("stop").disabled = !r;
  if (wasRecording && !r) loadFiles();
  wasRecording = !!r;
}
let wasRecording = false;

// ---- session view ----------------------------------------------------------

let lastSession = null;
let lastOffer = null;
let lastDodge = "Invalid";
let timerEnd = null;

function renderSession(s) {
  lastSession = s;
  const root = $("session");
  root.replaceChildren();
  if (!s) {
    root.className = "muted";
    root.textContent = "No champ select session.";
    timerEnd = null;
    return;
  }
  root.className = "";

  const acting = new Set();
  for (const group of s.actions || []) for (const a of group) if (a.isInProgress) acting.add(a.actorCellId);
  const current = (s.actions || []).flat().filter((a) => a.isInProgress);

  const t = s.timer || {};
  timerEnd = t.isInfinite ? null : Date.now() + (t.adjustedTimeLeftInPhase || 0);
  const timer = el("div", "timer");
  timer.append(el("span", "big", ""), el("span", "", t.phase || "?"));
  const turn = current.length ? current.map((a) => `${a.type} by cell ${a.actorCellId}`).join(", ") : "no action in progress";
  timer.append(el("span", "muted", turn));
  if (s.queueId != null) timer.append(el("span", "muted", `queue ${s.queueId}`));
  root.append(timer);

  if (Array.isArray(lastOffer) && lastOffer.length) root.append(offerRow(lastOffer, s));
  // ARAM-style modes: the shared bench, plus rerolls when allowed.
  if (s.benchEnabled) root.append(benchRow(s));

  const bans = s.bans || {};
  const teams = el("div", "teams");
  teams.append(
    teamColumn("Your team", "ally", s.myTeam || [], bans.myTeamBans || [], s.localPlayerCellId, acting),
    teamColumn("Enemy team", "enemy", s.theirTeam || [], bans.theirTeamBans || [], null, acting),
  );
  root.append(teams);
  tickTimer();
}

function offerRow(offer, s) {
  const me = (s.myTeam || []).find((p) => p.cellId === s.localPlayerCellId);
  const row = el("div", "bans offer");
  row.append(el("b", "", `Offered (${offer.length})`));
  for (const id of offer) {
    const chip = el("span", "ban", champ(id) || "none");
    if (me && me.championId === id) chip.classList.add("picked");
    row.append(chip);
  }
  return row;
}

function benchRow(s) {
  const bench = s.benchChampions || [];
  const row = el("div", "bans bench");
  row.append(el("b", "", `Bench (${bench.length})`));
  if (bench.length === 0) row.append(el("span", "muted", "empty"));
  for (const b of bench) {
    const chip = el("span", b.isPriority ? "ban priority" : "ban", champ(b.championId) || "none");
    if (b.isPriority) chip.title = "priority";
    row.append(chip);
  }
  if (s.allowRerolling) row.append(el("span", "muted", `rerolls left: ${s.rerollsRemaining ?? 0}`));
  return row;
}

function teamColumn(title, cls, players, bans, me, acting) {
  const col = el("div", cls);
  col.append(el("b", "", title));
  const banRow = el("div", "bans");
  if (bans.length === 0) banRow.append(el("span", "muted", "no bans"));
  for (const b of bans) banRow.append(el("span", "ban", champ(b) || "none"));
  col.append(banRow);
  for (const p of players) {
    const cell = el("div", "cell");
    if (p.cellId === me) cell.classList.add("me");
    if (acting.has(p.cellId)) cell.classList.add("acting");
    const pos = p.assignedPosition || "—";
    cell.append(el("span", "pos", `${pos}${p.isAutofilled ? " (fill)" : ""}`));
    const name = el("span", "champ", champ(p.championId) || "");
    if (!p.championId) {
      const hover = champ(p.championPickIntent);
      name.append(el("span", "hover", hover ? `hovering ${hover}` : "–"));
    }
    cell.append(name);
    cell.append(el("span", "pos", `cell ${p.cellId}${p.cellId === me ? " · you" : ""}`));
    col.append(cell);
  }
  return col;
}

function tickTimer() {
  const big = document.querySelector(".timer .big");
  if (!big) return;
  big.textContent = timerEnd ? `${Math.max(0, Math.ceil((timerEnd - Date.now()) / 1000))}s` : "∞";
}
setInterval(tickTimer, 250);

// ---- event log -------------------------------------------------------------

const logEntries = [];
const expanded = new Set();
let nextId = 0;

// RawEvent.t is on the source's clock: Unix ms when live, ms since the start
// of the recording when replayed. Anything past 2001 is a wall-clock time.
const formatT = (t) => (t > 1e12 ? new Date(t).toLocaleTimeString() : `+${(t / 1000).toFixed(1)}s`);

function addLog(event, problem) {
  logEntries.push({ id: nextId++, event, problem });
  if (logEntries.length > MAX_LOG) expanded.delete(logEntries.shift().id);
  renderLog();
}

function renderLog() {
  const filter = $("filter").value.trim();
  const problemsOnly = $("problems-only").checked;
  const log = $("log");
  const stick = log.scrollTop + log.clientHeight >= log.scrollHeight - 5;
  log.replaceChildren();
  let shown = 0;
  for (const { id, event, problem } of logEntries) {
    if (filter && !event.uri.includes(filter)) continue;
    if (problemsOnly && !problem) continue;
    shown++;
    const row = el("div", problem ? "ev problem" : "ev");
    row.append(el("span", "meta", `${formatT(event.t)} ${event.kind.padEnd(8)} `), document.createTextNode(event.uri));
    if (problem) row.append(el("div", "", `⚠ ${problem}`));
    if (expanded.has(id)) row.append(el("pre", "", JSON.stringify(event.data, null, 2)));
    // Rows are rebuilt on every event, so open/closed state lives in `expanded`.
    row.addEventListener("click", (e) => {
      if (e.target.tagName === "PRE") return;
      if (expanded.has(id)) expanded.delete(id);
      else expanded.add(id);
      renderLog();
    });
    log.append(row);
  }
  $("log-count").textContent = `${shown} of ${logEntries.length} shown (keeps last ${MAX_LOG})`;
  if (stick) log.scrollTop = log.scrollHeight;
}

$("filter").addEventListener("input", renderLog);
$("problems-only").addEventListener("change", renderLog);
$("clear").addEventListener("click", () => { logEntries.length = 0; expanded.clear(); renderLog(); });

// ---- notices & files -------------------------------------------------------

function notice(level, message) {
  const n = el("div", level, `${new Date().toLocaleTimeString()} ${message}`);
  $("notices").prepend(n);
  while ($("notices").children.length > 8) $("notices").lastChild.remove();
}

async function loadFiles() {
  const body = $("files").tBodies[0];
  try {
    const files = await (await fetch("/api/captures")).json();
    body.replaceChildren();
    if (files.length === 0) {
      const tr = el("tr");
      tr.append(el("td", "muted", "no recordings yet"));
      body.append(tr);
    }
    for (const f of files) {
      const tr = el("tr");
      const name = el("td");
      const a = el("a", "", f.file);
      a.href = `/api/captures/${encodeURIComponent(f.file)}`;
      name.append(a);
      tr.append(name, el("td", "num", `${(f.bytes / 1024).toFixed(1)} KB`), el("td", "num", new Date(f.modified * 1000).toLocaleString()));
      body.append(tr);
    }
  } catch (e) {
    body.replaceChildren();
    const tr = el("tr");
    tr.append(el("td", "error", `couldn't list files: ${e}`));
    body.append(tr);
  }
}

async function api(method, path, body) {
  const resp = await fetch(path, {
    method,
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!resp.ok) {
    const err = await resp.json().catch(() => ({ error: resp.statusText }));
    notice("error", err.error || resp.statusText);
  }
}

$("start").addEventListener("click", () => api("POST", "/api/capture/start", { label: $("label").value || "manual" }));
$("stop").addEventListener("click", () => api("POST", "/api/capture/stop"));
$("auto").addEventListener("change", (e) => api("PUT", "/api/capture/auto", { on: e.target.checked }));
$("refresh").addEventListener("click", loadFiles);

// ---- websocket -------------------------------------------------------------

function connect() {
  const ws = new WebSocket(`ws://${location.host}/api/events`);
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.type === "status") renderStatus(msg);
    else if (msg.type === "notice") notice(msg.level, msg.message);
    else if (msg.type === "event") {
      addLog(msg.event, msg.problem);
      const { uri, kind, data } = msg.event;
      if (SESSION_URIS.includes(uri)) renderSession(kind === "Delete" ? null : data);
      else if (uri === SUBSET_URI) {
        lastOffer = kind === "Delete" ? null : data;
        if (lastSession) renderSession(lastSession);
      } else if (uri === MATCHMAKING_URI) {
        const state = (kind !== "Delete" && data && data.dodgeData && data.dodgeData.state) || "Invalid";
        if (state !== lastDodge && state !== "Invalid") notice("warn", `champ select ended early: ${state}`);
        lastDodge = state;
      }
    }
  };
  ws.onclose = () => {
    $("conn-dot").className = "dot err";
    $("conn").textContent = "devserver connection lost, retrying…";
    setTimeout(connect, 1500);
  };
}

connect();
loadFiles();
