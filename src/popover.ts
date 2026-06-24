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
  if (m.last_checked) parts.push(m.last_checked);
  meta.textContent = parts.join(" · ");

  body.append(name, meta);
  li.append(dot, body);

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
  summary.textContent =
    `${up} up · ${down} down` + (unknown ? ` · ${unknown} unknown` : "");

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
  draw();
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
  await listen<MonitorView[]>("monitors:updated", (e) => setMonitors(e.payload));
});
