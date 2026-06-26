import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

// Inline latency sparkline (SVG polyline) from per-bucket avg-ms history.
function sparkline(history: number[]): SVGSVGElement | null {
  if (history.length < 2) return null;
  const w = 56;
  const h = 14;
  const min = Math.min(...history);
  const max = Math.max(...history);
  const span = max - min || 1;
  const pts = history
    .map((v, i) => {
      const x = (i / (history.length - 1)) * w;
      const y = h - ((v - min) / span) * h; // higher latency = higher line
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "spark");
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));
  const line = document.createElementNS(ns, "polyline");
  line.setAttribute("points", pts);
  svg.append(line);
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

const statusRank = (s: Status) =>
  s === "down" ? 0 : s === "unknown" ? 1 : s === "paused" ? 2 : 3;

const byStatusThenName = (a: MonitorView, b: MonitorView) =>
  statusRank(a.status) - statusRank(b.status) ||
  a.name.toLowerCase().localeCompare(b.name.toLowerCase());

const providerId = (m: MonitorView) => m.key.slice(0, m.key.indexOf(":"));

let current: MonitorView[] = [];
let groupMode: "status" | "provider" =
  localStorage.getItem("uptimebar.group") === "provider" ? "provider" : "status";
let filterMode: "all" | "problems" =
  localStorage.getItem("uptimebar.filter") === "problems" ? "problems" : "all";

// "Problems" = anything not healthy/paused: down or unknown (degraded/unreachable).
const isProblem = (m: MonitorView) => m.status === "down" || m.status === "unknown";

// Tier-3 on-demand detail (latency, uptime %) keyed by monitor `key`. Populated
// when the popover opens by fetching each provider's detail endpoint; absent for
// providers without a detail tier.
interface Detail {
  latencyMs?: number;
  uptimePct?: number;
  history?: number[]; // per-bucket avg latency, for a sparkline
}
const detail = new Map<string, Detail>();

function monitorRow(m: MonitorView): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "monitor";
  li.style.borderLeftColor = kindColor(m.provider_kind);

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
  // Prefer "down for Xm" / "up for Xh" (from state_since) over a raw timestamp;
  // it's the more useful signal and survives the ETag 304 steady state.
  const dur = durationLabel(m);
  if (dur) parts.push(dur);
  else if (m.last_checked) parts.push(m.last_checked);
  // Tier-3 detail (latency, uptime), enriched on demand when popover opens.
  const det = detail.get(m.key);
  if (det?.latencyMs != null) parts.push(`${det.latencyMs} ms`);
  if (det?.uptimePct != null) parts.push(`${det.uptimePct.toFixed(det.uptimePct >= 100 ? 0 : 1)}%`);
  meta.textContent = parts.join(" · ");

  body.append(name, meta);
  li.append(dot, body);

  // Latency sparkline (Tier-3), right-aligned, when history is available.
  if (det?.history) {
    const spark = sparkline(det.history);
    if (spark) li.append(spark);
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

  const dot = document.createElement("span");
  dot.className = "gh-dot";
  dot.style.background = kindColor(kind);

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

function draw() {
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
    // Services with outages first, then alphabetical.
    const ordered = [...groups.values()].sort((a, b) => {
      const ad = a.items.some((m) => m.status === "down") ? 0 : 1;
      const bd = b.items.some((m) => m.status === "down") ? 0 : 1;
      return ad - bd || a.label.toLowerCase().localeCompare(b.label.toLowerCase());
    });
    for (const g of ordered) {
      if (g.shown.length === 0) continue; // hide groups with nothing to show
      list.append(groupHeader(g.label, g.kind, g.items));
      for (const m of [...g.shown].sort(byStatusThenName)) list.append(monitorRow(m));
    }
  } else {
    for (const m of [...visible].sort(byStatusThenName)) list.append(monitorRow(m));
  }
}

function setMonitors(monitors: MonitorView[]) {
  current = monitors;
  lastSync = Date.now();
  draw();
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

function updateToggleLabel() {
  const btn = document.getElementById("group-toggle");
  if (!btn) return;
  btn.textContent = groupMode === "provider" ? "Ungroup" : "Group";
  btn.title =
    groupMode === "provider" ? "Show a flat list (by status)" : "Group monitors by service";
}

function updateFilterLabel() {
  const btn = document.getElementById("filter-toggle");
  if (!btn) return;
  const on = filterMode === "problems";
  btn.textContent = on ? "Problems" : "All";
  btn.title = on ? "Showing only down/degraded — click to show all" : "Show only down/degraded monitors";
  btn.classList.toggle("active", on);
}

async function refresh() {
  setMonitors(await invoke<MonitorView[]>("get_monitors"));
}

// Tier-3: fetch rich detail (latency, uptime %) for each provider on demand and
// merge it into `detail`, then redraw. Best-effort — failures are silent (the
// always-on status tier already populated the list). Runs when the popover opens.
async function loadDetail() {
  const ids = new Set(current.map(providerId));
  await Promise.all(
    [...ids].map(async (pid) => {
      try {
        const data = await invoke<any>("get_provider_detail", { providerId: pid });
        const monitors = data?.monitors;
        if (!Array.isArray(monitors)) return;
        for (const md of monitors) {
          if (md?.id == null) continue;
          const key = `${pid}:${md.id}`;
          // Field names verified against the live /api/v1/dashboard/ response.
          const d: Detail = {};
          if (typeof md.latest_response_time_ms === "number")
            d.latencyMs = Math.round(md.latest_response_time_ms);
          if (typeof md.uptime_pct === "number") d.uptimePct = md.uptime_pct;
          if (Array.isArray(md.response_history)) {
            const buckets = md.response_history
              .map((b: any) => (typeof b?.avg_ms === "number" ? b.avg_ms : null))
              .filter((v: number | null): v is number => v != null);
            if (buckets.length) d.history = buckets;
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
  // Esc dismisses the popover (Mac norm for transient panels, like Control Center).
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") invoke("close_popover");
  });

  document.getElementById("refresh")?.addEventListener("click", () => invoke("refresh_now"));
  document.getElementById("settings")?.addEventListener("click", () => invoke("open_settings"));
  document.getElementById("group-toggle")?.addEventListener("click", () => {
    groupMode = groupMode === "provider" ? "status" : "provider";
    localStorage.setItem("uptimebar.group", groupMode);
    updateToggleLabel();
    draw();
  });
  document.getElementById("filter-toggle")?.addEventListener("click", () => {
    filterMode = filterMode === "problems" ? "all" : "problems";
    localStorage.setItem("uptimebar.filter", filterMode);
    updateFilterLabel();
    draw();
  });

  updateToggleLabel();
  updateFilterLabel();
  await refresh();
  await loadDetail();
  // The popover is hidden/shown (not reloaded); refresh detail each time a human
  // brings it to the front, so latency/uptime are current when viewed.
  window.addEventListener("focus", () => {
    void loadDetail();
  });
  await listen<MonitorView[]>("monitors:updated", (e) => setMonitors(e.payload));
});
