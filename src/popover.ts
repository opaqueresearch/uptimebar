import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ICONS, mkIcon } from "./icons";

type Status = "up" | "down" | "paused" | "unknown";

interface MonitorView {
  key: string;
  provider_label: string;
  provider_kind: string;
  name: string;
  status: Status;
  last_checked: string | null;
  url: string | null;
  detail_url: string | null;
  state_since: string | null;
  provider_color: string | null;
  public_id: string | null;
  is_muted: boolean;
  can_pause: boolean; // provider supports pause/resume
  can_mute: boolean; // provider supports mute/unmute (Watch4.me only)
  token_scope: string; // "write" | "read" | "unknown"
}

// Compact human duration since an ISO timestamp, e.g. "14m", "3h", "2d".
function since(iso: string | null): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const secs = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  return `${Math.floor(hrs / 24)}d`;
}

// "down for Xm" / "up for Xh" from state_since, when the provider reports it.
function durationLabel(m: MonitorView): string | null {
  const d = since(m.state_since);
  if (!d) return null;
  if (m.status === "down") return `down for ${d}`;
  if (m.status === "up") return `up for ${d}`;
  return null;
}

// Inline latency sparkline from per-bucket avg-ms history; buckets that had
// failures get a red dot, mirroring the watch4.me dashboard's segment coloring.
function sparkline(history: HistoryPoint[]): SVGSVGElement | null {
  if (history.length < 2) return null;
  const w = 56;
  const h = 14;
  const vals = history.map((p) => p.avgMs);
  const min = Math.min(...vals);
  const max = Math.max(...vals);
  const span = max - min || 1;
  const xy = (i: number, v: number): [number, number] => [
    (i / (history.length - 1)) * w,
    h - ((v - min) / span) * h, // higher latency = higher line
  ];
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "spark");
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));

  const line = document.createElementNS(ns, "polyline");
  line.setAttribute(
    "points",
    history.map((p, i) => xy(i, p.avgMs).map((n) => n.toFixed(1)).join(",")).join(" "),
  );
  svg.append(line);

  // Mark failure buckets so an outage in the window is visible at a glance.
  history.forEach((p, i) => {
    if (p.failures > 0) {
      const [cx, cy] = xy(i, p.avgMs);
      const dot = document.createElementNS(ns, "circle");
      dot.setAttribute("class", "spark-fail");
      dot.setAttribute("cx", cx.toFixed(1));
      dot.setAttribute("cy", cy.toFixed(1));
      dot.setAttribute("r", "1.6");
      svg.append(dot);
    }
  });
  return svg;
}

// Per-service display name + accent color, so each row/group is identifiable.
const KIND_META: Record<string, { name: string; color: string }> = {
  watch4me: { name: "Watch4.me", color: "#3b82f6" },
  healthchecks: { name: "Healthchecks.io", color: "#30a46c" },
  betterstack: { name: "BetterStack", color: "#a855f7" },
  uptimerobot: { name: "UptimeRobot", color: "#f59e0b" },
  uptimekuma: { name: "Uptime Kuma", color: "#14b8a6" },
};
const kindName = (k: string) => KIND_META[k]?.name ?? k;
const kindColor = (k: string) => KIND_META[k]?.color ?? "#8b8d98";

// Resolve a monitor's bar color: the provider's chosen color (a hex string from
// the Settings palette — Apple tag colors, defined in settings.ts) if set, else
// the kind default.
const barColor = (m: MonitorView) => m.provider_color || kindColor(m.provider_kind);

const statusRank = (s: Status) =>
  s === "down" ? 0 : s === "unknown" ? 1 : s === "paused" ? 2 : 3;

const byStatusThenName = (a: MonitorView, b: MonitorView) =>
  statusRank(a.status) - statusRank(b.status) ||
  a.name.toLowerCase().localeCompare(b.name.toLowerCase());

const providerId = (m: MonitorView) => m.key.slice(0, m.key.indexOf(":"));
// The native monitor id (the key is "{providerId}:{monitorId}"). Actions key on it.
const monitorId = (m: MonitorView) => m.key.slice(m.key.indexOf(":") + 1);

let current: MonitorView[] = [];

// Frozen display order: captured when the popover opens/refreshes and held stable
// the whole time it's open, so acting on a monitor (or a live status change) never
// reshuffles rows under the user — the status DOT recolors in place instead. The
// map is monitor-key -> sort index; new keys not in it sort after, by status+name.
let orderIndex = new Map<string, number>();
const orderOf = (m: MonitorView) => orderIndex.get(m.key) ?? Number.MAX_SAFE_INTEGER;

// Sort by the frozen order first, then status+name for any not-yet-captured rows.
const byFrozenOrder = (a: MonitorView, b: MonitorView) =>
  orderOf(a) - orderOf(b) || byStatusThenName(a, b);

// Recompute the frozen order from `current` using the live status+name sort. Called
// only on open/refresh — never on a background `monitors:updated`.
function captureOrder() {
  const sorted = [...current].sort(byStatusThenName);
  orderIndex = new Map(sorted.map((m, i) => [m.key, i]));
}
let groupMode: "status" | "provider" =
  localStorage.getItem("uptimebar.group") === "provider" ? "provider" : "status";
let filterMode: "all" | "problems" =
  localStorage.getItem("uptimebar.filter") === "problems" ? "problems" : "all";

// "Problems" = anything not healthy/paused: down or unknown (degraded/unreachable).
const isProblem = (m: MonitorView) => m.status === "down" || m.status === "unknown";

// Tier-3 on-demand detail (latency, uptime %) keyed by monitor `key`. Populated
// when the popover opens by fetching each provider's detail endpoint; absent for
// providers without a detail tier.
interface HistoryPoint {
  avgMs: number;
  failures: number; // >0 -> color that segment as a failure
}
interface Detail {
  latencyMs?: number; // kept as a float (e.g. 906.26); rounded only at display
  uptimePct?: number;
  history?: HistoryPoint[];
}
const detail = new Map<string, Detail>();

// Transient inline banner (the popover has no toast). Auto-hides after a few
// seconds; the newest message replaces any prior one.
let noticeTimer: number | undefined;
function showNotice(msg: string) {
  const el = document.getElementById("notice") as HTMLElement | null;
  if (!el) return;
  el.textContent = msg;
  el.hidden = false;
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    el.hidden = true;
  }, 6000);
}

// Which CONTROL on a row has an action in flight — so ONLY the clicked button
// spins, not its sibling. Keyed by monitor key → "pause" (pause/resume control) or
// "mute" (mute/unmute control). State (paused/muted) itself is NOT tracked locally;
// it comes from the backend's monitors:updated emit after the action succeeds, so
// the UI never shows a state the server rejected.
type Control = "pause" | "mute";
const actionPending = new Map<string, Control>();
const pendingControl = (key: string): Control | undefined => actionPending.get(key);

// Fire a monitor action (pause/resume/mute/unmute). Shows a pending spinner on the
// clicked control immediately (instant feedback despite round-trip lag). On success
// the backend emits monitors:updated → re-render with the real new state. On failure
// NOTHING local changes — the transient notice explains it and the row is unchanged.
async function runAction(m: MonitorView, action: "pause" | "resume" | "mute" | "unmute") {
  const control: Control = action === "pause" || action === "resume" ? "pause" : "mute";
  actionPending.set(m.key, control);
  draw(); // show the pending spinner right away (order is frozen, no reshuffle)
  try {
    await invoke("monitor_action", {
      providerId: providerId(m),
      monitorId: monitorId(m),
      action,
      durationSecs: action === "mute" ? muteDurationSecs() : null,
    });
    // Success: the backend emit re-renders with the new paused/muted state.
  } catch (e) {
    if (String(e).includes("insufficient_scope")) {
      showNotice("Read-only token — add a read+write token in Settings to control monitors.");
    } else {
      showNotice(`Action failed: ${e}`);
    }
    // No local state change on failure — the row stays exactly as it was.
  } finally {
    actionPending.delete(m.key);
    draw();
  }
}

// Build one action button using the two-channel encoding:
//   • icon SHAPE  = state (caller picks play/pause, bell/bell-slash)
//   • .engaged    = the action is currently on → filled chip + always-visible
//   • .read-only  = not clickable (dim + not-allowed cursor), but still shows state
//   • .pending    = in flight (spinner)
// A non-engaged button on a write token is hover-revealed; engaged (or read-only
// showing a state) stays visible. No lingering "failed" color — failures surface
// in the transient notice banner instead.
function actionButton(o: {
  icon: string;
  engaged: boolean;
  pending: boolean;
  readOnly: boolean;
  title: string;
  onClick: () => void;
}): HTMLButtonElement {
  const b = mkIcon(o.icon, o.title);
  b.classList.toggle("engaged", o.engaged);
  b.classList.toggle("pending", o.pending);
  if (o.pending) b.classList.add("spinning");
  if (o.readOnly) {
    b.classList.add("read-only");
    b.setAttribute("aria-disabled", "true");
  }
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    if (o.readOnly || o.pending) return; // non-interactive
    o.onClick();
  });
  return b;
}

// The default mute duration configured for the (Watch4.me) provider, in seconds,
// or null for indefinite. Populated from get_providers on load / config change.
let muteDefaultSecs: number | null = null;
const muteDurationSecs = () => muteDefaultSecs;

async function loadMuteDefault() {
  try {
    const configs = await invoke<Array<{ kind: string; mute_default_secs: number | null }>>(
      "get_providers",
    );
    const w4m = configs.find((c) => c.kind === "watch4me");
    muteDefaultSecs = w4m?.mute_default_secs ?? null;
  } catch {
    muteDefaultSecs = null;
  }
}

function monitorRow(m: MonitorView): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "monitor";
  // In grouped mode the row joins its header's bold left band (5px, same color);
  // in flat mode the thinner 3px bar is the only per-service marker.
  if (groupMode === "provider") li.classList.add("grouped");
  li.style.borderLeftColor = barColor(m);

  const dot = document.createElement("span");
  dot.className = `dot ${m.status}`;

  const body = document.createElement("div");
  body.className = "monitor-body";

  const name = document.createElement("div");
  name.className = "monitor-name";
  name.textContent = m.name;

  const meta = document.createElement("div");
  meta.className = "monitor-meta";
  const parts: string[] = [];
  // In a flat list, name the service per row; when grouped the header carries it.
  if (groupMode === "status") parts.push(kindName(m.provider_kind));
  // Prefer "down for Xm" / "up for Xh" (from state_since) over a last-checked time;
  // it's the more useful signal and survives the ETag 304 steady state. When a
  // provider has no state_since (e.g. BetterStack), fall back to a *relative*
  // "checked Xm ago" rather than dumping the raw ISO UTC string.
  const dur = durationLabel(m);
  if (dur) parts.push(dur);
  else if (m.last_checked) {
    const ago = since(m.last_checked);
    parts.push(ago ? `checked ${ago} ago` : m.last_checked);
  }
  // Tier-3 detail (latency, uptime), enriched on demand when popover opens.
  const det = detail.get(m.key);
  if (det?.latencyMs != null) parts.push(`${Math.round(det.latencyMs)} ms`);
  if (det?.uptimePct != null) parts.push(`${det.uptimePct.toFixed(det.uptimePct >= 100 ? 0 : 1)}%`);
  meta.textContent = parts.join(" · ");

  body.append(name, meta);
  li.append(dot, body);

  // Latency sparkline (Tier-3), right-aligned, when history is available.
  if (det?.history) {
    const spark = sparkline(det.history);
    if (spark) li.append(spark);
  }

  // Action buttons (Watch4.me, write-scoped). Each button IS the state indicator
  // for its action AND the way to change it — one control, always in the same
  // place. Normally hover-revealed; but a button whose action is "engaged"
  // (paused/muted), in flight (pending), or just failed STAYS visible + styled
  // even un-hovered, so you can see and undo state without hunting. stopPropagation
  // keeps a button click from also triggering the row's open-in-browser handler.
  if (m.can_pause || m.can_mute) {
    const actions = document.createElement("div");
    actions.className = "monitor-actions";
    const busy = pendingControl(m.key); // which control is in flight, if any
    // Read-only token: buttons are non-interactive but still SHOW state
    // (a read-only user can see paused/muted without visiting the website).
    const readOnly = m.token_scope === "read";

    // Pause/resume — shown for any provider that supports it. Icon SHAPE encodes
    // state (play=paused, pause=running); opacity/cursor = actionability.
    if (m.can_pause) {
      const paused = m.status === "paused";
      const pausePending = busy === "pause";
      actions.append(
        actionButton({
          icon: pausePending ? ICONS.spinner : paused ? ICONS.play : ICONS.pause,
          engaged: paused,
          pending: pausePending,
          readOnly,
          title: readOnly
            ? paused
              ? "Paused — read+write token needed to change"
              : "Read+write token needed to pause"
            : paused
              ? "Resume"
              : "Pause",
          onClick: () => runAction(m, paused ? "resume" : "pause"),
        }),
      );
    }

    // Mute/unmute — Watch4.me only. bell-slash=muted, bell=alerting.
    if (m.can_mute) {
      const mutePending = busy === "mute";
      actions.append(
        actionButton({
          icon: mutePending ? ICONS.spinner : m.is_muted ? ICONS.mute : ICONS.unmute,
          engaged: m.is_muted,
          pending: mutePending,
          readOnly,
          title: readOnly
            ? m.is_muted
              ? "Muted — read+write token needed to change"
              : "Read+write token needed to mute"
            : m.is_muted
              ? "Unmute"
              : "Mute",
          onClick: () => runAction(m, m.is_muted ? "unmute" : "mute"),
        }),
      );
    }

    li.append(actions);
  }

  if (m.detail_url) {
    const url = m.detail_url;
    li.classList.add("clickable");
    li.title = `Open ${m.name} in browser`;
    const hint = document.createElement("span");
    hint.className = "open-hint";
    hint.textContent = "↗";
    li.append(hint);
    li.addEventListener("click", () => invoke("open_url", { url }));
  }
  return li;
}

function groupHeader(label: string, kind: string, items: MonitorView[]): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "group-header";
  // All rows in a group share a provider, so take the chosen color off the first.
  const color = items.length ? barColor(items[0]) : kindColor(kind);
  // Bold left color band = provider identity, continuous with its rows below.
  li.style.borderLeftColor = color;

  const dot = document.createElement("span");
  dot.className = "gh-dot";
  dot.style.background = color;

  const lbl = document.createElement("span");
  lbl.className = "gh-label";
  lbl.textContent = label;

  const sub = document.createElement("span");
  sub.className = "gh-kind";
  // Avoid "Watch4.me — Watch4.me" when the label wasn't customized.
  sub.textContent = kindName(kind) === label ? "" : kindName(kind);

  const down = items.filter((m) => m.status === "down").length;
  const count = document.createElement("span");
  count.className = "gh-count";
  count.textContent = down ? `${items.length} · ${down} down` : `${items.length}`;
  if (down) count.classList.add("has-down");

  li.append(dot, lbl, sub, count);
  return li;
}

// Render the list, then fit the window to it. `draw` wraps `drawList` so the
// content-fit runs on every render path (including the early returns inside).
function draw() {
  drawList();
  fitWindow();
}

function drawList() {
  const list = document.getElementById("list") as HTMLUListElement;
  const empty = document.getElementById("empty") as HTMLElement;
  const summary = document.getElementById("summary") as HTMLElement;

  list.innerHTML = "";

  // Summary always reflects ALL monitors, even when the list is filtered.
  let up = 0;
  let down = 0;
  let unknown = 0;
  for (const m of current) {
    if (m.status === "up") up++;
    else if (m.status === "down") down++;
    else if (m.status === "unknown") unknown++;
  }

  if (current.length === 0) {
    empty.textContent = "No monitors yet. Open Settings to add a provider.";
    empty.hidden = false;
    summary.textContent = "No monitors";
    return;
  }
  const counts = `${up} up · ${down} down` + (unknown ? ` · ${unknown} unknown` : "");
  summary.textContent = counts;
  // Own-clock freshness, shown subtly after the counts.
  const synced = syncedLabel();
  if (synced) {
    const s = document.createElement("span");
    s.className = "synced";
    s.textContent = ` · ${synced}`;
    summary.append(s);
  }

  // Problems filter: render only down/unknown rows, but keep group counts honest.
  const visible = filterMode === "problems" ? current.filter(isProblem) : current;

  if (visible.length === 0) {
    // Filtered to problems, but nothing is wrong — the best possible answer.
    empty.textContent = "All systems operational ✓";
    empty.hidden = false;
    return;
  }
  empty.hidden = true;

  if (groupMode === "provider") {
    // Group on the full set (for accurate header counts), render visible rows.
    const groups = new Map<
      string,
      { label: string; kind: string; items: MonitorView[]; shown: MonitorView[] }
    >();
    for (const m of current) {
      const pid = providerId(m);
      let g = groups.get(pid);
      if (!g) {
        g = { label: m.provider_label, kind: m.provider_kind, items: [], shown: [] };
        groups.set(pid, g);
      }
      g.items.push(m);
      if (filterMode === "all" || isProblem(m)) g.shown.push(m);
    }
    // Frozen order: a group sorts by the earliest frozen index among its monitors
    // (the group that was on top when captured stays on top), so groups don't
    // reshuffle mid-interaction either. Rows within a group use the same frozen order.
    const groupRank = (items: MonitorView[]) => Math.min(...items.map(orderOf));
    const ordered = [...groups.values()].sort(
      (a, b) =>
        groupRank(a.items) - groupRank(b.items) ||
        a.label.toLowerCase().localeCompare(b.label.toLowerCase()),
    );
    for (const g of ordered) {
      if (g.shown.length === 0) continue; // hide groups with nothing to show
      list.append(groupHeader(g.label, g.kind, g.items));
      for (const m of [...g.shown].sort(byFrozenOrder)) list.append(monitorRow(m));
    }
  } else {
    for (const m of [...visible].sort(byFrozenOrder)) list.append(monitorRow(m));
  }
}

function setMonitors(monitors: MonitorView[]) {
  current = monitors;
  lastSync = Date.now();
  draw();
}

// Fit the native window to the popover's natural content height. The list uses
// flex:1 + overflow, so its rendered offsetHeight is clamped to the current
// window; `scrollHeight` gives the true content height instead. Rust clamps the
// final value to [MIN, MAX] and re-anchors to the tray icon. Called after draw().
function fitWindow() {
  const header = document.querySelector(".popover-header") as HTMLElement | null;
  const list = document.getElementById("list") as HTMLElement | null;
  const empty = document.getElementById("empty") as HTMLElement | null;
  if (!header) return;
  const listH = list && !list.hidden ? list.scrollHeight : 0;
  const emptyH = empty && !empty.hidden ? empty.offsetHeight : 0;
  // +2 guards against sub-pixel rounding that would otherwise show a scrollbar.
  const height = header.offsetHeight + listH + emptyH + 2;
  void invoke("resize_popover", { height });
}

// Own-clock freshness: the team's status endpoint excludes latest_check_at from
// the ETag, so during 304 steady-state we show "synced Xs ago" from our own last
// successful sync, not a per-monitor timestamp.
let lastSync = 0;
function syncedLabel(): string {
  if (!lastSync) return "";
  const secs = Math.floor((Date.now() - lastSync) / 1000);
  if (secs < 5) return "synced just now";
  if (secs < 60) return `synced ${secs}s ago`;
  return `synced ${Math.floor(secs / 60)}m ago`;
}

// Reflect the active segment in each segmented control (shaded = selected).
function updateSegments() {
  document.querySelectorAll<HTMLElement>("#group-seg .seg").forEach((b) => {
    b.classList.toggle("active", b.dataset.group === groupMode);
  });
  document.querySelectorAll<HTMLElement>("#filter-seg .seg").forEach((b) => {
    b.classList.toggle("active", b.dataset.filter === filterMode);
  });
}

// A full refresh (initial load, manual ⌘R, or popover re-shown) re-captures the
// frozen order, then draws. Background `monitors:updated` events use setMonitors
// directly, which does NOT re-capture — so live changes recolor rows in place.
async function refresh() {
  current = await invoke<MonitorView[]>("get_monitors");
  lastSync = Date.now();
  captureOrder();
  draw();
}

// Tier-3: fetch rich detail (latency, uptime %) for each provider on demand and
// merge it into `detail`, then redraw. Best-effort — failures are silent (the
// always-on status tier already populated the list). Runs when the popover opens.
// `force` bypasses the native per-provider detail cache (manual refresh); the
// open/focus path leaves it false so the cache gates repeated opens.
async function loadDetail(force = false) {
  const ids = new Set(current.map(providerId));
  await Promise.all(
    [...ids].map(async (pid) => {
      try {
        const data = await invoke<any>("get_provider_detail", { providerId: pid, force });
        const monitors = data?.monitors;
        if (!Array.isArray(monitors)) return;
        for (const md of monitors) {
          if (md?.id == null) continue;
          const key = `${pid}:${md.id}`;
          // Field names + types per the Watch4.me dashboard contract.
          // latest_response_time_ms is a FLOAT — keep it, round only at display.
          const d: Detail = {};
          if (typeof md.latest_response_time_ms === "number")
            d.latencyMs = md.latest_response_time_ms;
          if (typeof md.uptime_pct === "number") d.uptimePct = md.uptime_pct;
          if (Array.isArray(md.response_history)) {
            const pts: HistoryPoint[] = md.response_history
              .filter((b: any) => typeof b?.avg_ms === "number")
              .map((b: any) => ({
                avgMs: b.avg_ms,
                failures: typeof b.failures === "number" ? b.failures : 0,
              }));
            if (pts.length) d.history = pts;
          }
          if (d.latencyMs != null || d.uptimePct != null || d.history) detail.set(key, d);
        }
      } catch {
        // No detail tier / transient error — leave the status-only rows as-is.
      }
    }),
  );
  draw();
}

window.addEventListener("DOMContentLoaded", async () => {
  // Tell the native side whether the pointer is inside the popover, so it can
  // suppress the focus-loss auto-hide while we drag the popover's own scrollbar
  // (a scrollbar drag drops window focus on macOS but the pointer stays inside).
  document.addEventListener("mouseenter", () => invoke("set_pointer_inside", { inside: true }));
  document.addEventListener("mouseleave", () => invoke("set_pointer_inside", { inside: false }));

  // Keyboard shortcuts while the popover is focused. The tray-menu accelerators
  // (⌘,/⌘R) only fire when the menu has context, not when this webview is front,
  // so handle them here too.
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      invoke("close_popover");
    } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
      e.preventDefault();
      invoke("open_settings");
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
      e.preventDefault();
      invoke("refresh_now");
      void loadDetail(true);
    }
  });

  document.getElementById("refresh")?.addEventListener("click", () => {
    invoke("refresh_now");
    void loadDetail(true);
  });
  document.getElementById("settings")?.addEventListener("click", () => invoke("open_settings"));
  document.querySelectorAll<HTMLElement>("#group-seg .seg").forEach((b) => {
    b.addEventListener("click", () => {
      const next = b.dataset.group === "provider" ? "provider" : "status";
      if (next === groupMode) return;
      groupMode = next;
      localStorage.setItem("uptimebar.group", groupMode);
      updateSegments();
      draw();
    });
  });
  document.querySelectorAll<HTMLElement>("#filter-seg .seg").forEach((b) => {
    b.addEventListener("click", () => {
      const next = b.dataset.filter === "problems" ? "problems" : "all";
      if (next === filterMode) return;
      filterMode = next;
      localStorage.setItem("uptimebar.filter", filterMode);
      updateSegments();
      draw();
    });
  });

  updateSegments();
  await loadMuteDefault();
  await refresh();
  await loadDetail();
  // The popover is hidden/shown (not reloaded). Each time a human brings it to the
  // front, re-capture the frozen order (a fresh look re-sorts by current status)
  // and refresh detail, so latency/uptime + ordering are current when viewed.
  window.addEventListener("focus", () => {
    void refresh().then(() => loadDetail());
  });
  await listen<MonitorView[]>("monitors:updated", (e) => setMonitors(e.payload));
});
