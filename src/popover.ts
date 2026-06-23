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

function render(monitors: MonitorView[]) {
  const list = document.getElementById("list") as HTMLUListElement;
  const empty = document.getElementById("empty") as HTMLElement;
  const summary = document.getElementById("summary") as HTMLElement;

  list.innerHTML = "";

  if (monitors.length === 0) {
    empty.hidden = false;
    summary.textContent = "No monitors";
    return;
  }
  empty.hidden = true;

  let up = 0;
  let down = 0;
  let unknown = 0;
  for (const m of monitors) {
    if (m.status === "up") up++;
    else if (m.status === "down") down++;
    else if (m.status === "unknown") unknown++;

    const li = document.createElement("li");
    li.className = "monitor";

    const dot = document.createElement("span");
    dot.className = `dot ${m.status}`;

    const body = document.createElement("div");
    body.className = "monitor-body";

    const name = document.createElement("div");
    name.className = "monitor-name";
    name.textContent = m.name;

    const meta = document.createElement("div");
    meta.className = "monitor-meta";
    meta.textContent = m.last_checked
      ? `${m.provider_label} · ${m.last_checked}`
      : m.provider_label;

    body.append(name, meta);
    li.append(dot, body);
    list.append(li);
  }

  summary.textContent =
    `${up} up · ${down} down` + (unknown ? ` · ${unknown} unknown` : "");
}

async function refresh() {
  render(await invoke<MonitorView[]>("get_monitors"));
}

window.addEventListener("DOMContentLoaded", async () => {
  document
    .getElementById("refresh")
    ?.addEventListener("click", () => invoke("refresh_now"));
  document
    .getElementById("settings")
    ?.addEventListener("click", () => invoke("open_settings"));

  await refresh();
  await listen<MonitorView[]>("monitors:updated", (e) => render(e.payload));
});
