import { invoke } from "@tauri-apps/api/core";

interface ProviderConfig {
  id: string;
  kind: string;
  label: string;
  base_url: string | null;
  interval_secs: number | null;
  extra: string | null;
  color: string | null;
  // Server-owned; the form never sets it (upsert_provider preserves it).
  scope: string | null;
  // Per-provider default mute duration in seconds (null = indefinite).
  mute_default_secs: number | null;
}

// Shared provider-bar palette — Apple's Finder/tag label colors, used on both
// macOS and Windows. Keep in sync with PALETTE in popover.ts. Order = swatch order.
const PALETTE: { name: string; hex: string }[] = [
  { name: "Red", hex: "#e5484d" },
  { name: "Orange", hex: "#f5821f" },
  { name: "Yellow", hex: "#f5d90a" },
  { name: "Green", hex: "#30a46c" },
  { name: "Blue", hex: "#3b82f6" },
  { name: "Purple", hex: "#a855f7" },
  { name: "Pink", hex: "#e93d82" },
  { name: "Gray", hex: "#8b8d98" },
];

// Lucide icons (MIT), inlined as SVG so there's no dependency or network fetch.
// 16px, currentColor stroke — they inherit the button's text color. Paths copied
// verbatim from lucide.dev: refresh-cw (Test), pencil (Edit), trash-2 (Remove).
const svg = (paths: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" ` +
  `fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" ` +
  `stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

const ICONS = {
  test: svg(
    `<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>` +
      `<path d="M21 3v5h-5"/>` +
      `<path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>` +
      `<path d="M8 16H3v5"/>`,
  ),
  edit: svg(
    `<path d="M21.174 6.812a1 1 0 0 0-3.986-3.986L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>` +
      `<path d="m15 5 4 4"/>`,
  ),
  remove: svg(
    `<path d="M3 6h18"/>` +
      `<path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>` +
      `<path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>` +
      `<line x1="10" x2="10" y1="11" y2="17"/>` +
      `<line x1="14" x2="14" y1="11" y2="17"/>`,
  ),
};

interface ProviderMeta {
  kind: string;
  name: string;
  help: string;
  docs_url: string | null;
  default_base_url: string | null;
  base_url_placeholder: string;
  requires_base_url: boolean;
  requires_secret: boolean;
  secret_label: string;
  extra_label: string | null;
  extra_placeholder: string;
  extra_help: string;
}

let kinds: ProviderMeta[] = [];

// True while editing a provider that already has a saved key. The key value is
// never sent to the frontend (secrets are write-only), so when this is true and
// the field is empty there is nothing to reveal — the placeholder says "saved"
// and the Show button is disabled until the user types a replacement.
let editingHasSavedKey = false;

// Currently selected provider-bar color (hex), or null = use the kind default.
let selectedColor: string | null = null;

function el<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

// Per-kind default bar colors — must mirror KIND_META in popover.ts, so the
// "Default" chip and the current-color preview show what no-override resolves to.
const KIND_COLOR: Record<string, string> = {
  watch4me: "#3b82f6",
  healthchecks: "#30a46c",
  betterstack: "#a855f7",
  uptimerobot: "#f59e0b",
  uptimekuma: "#14b8a6",
};
const kindColor = (kind: string) => KIND_COLOR[kind] ?? "#8b8d98";

// The color currently in effect for the form's provider: the chosen override, or
// the kind default when none is set.
function effectiveColor(): string {
  return selectedColor ?? kindColor(el<HTMLSelectElement>("kind").value);
}

// Render the color-swatch picker into #color-swatches, reflecting `selectedColor`,
// plus a "Current" preview dot so it's obvious which color is active right now.
// A "Default" swatch (null) clears the override so the kind's default color wins.
function renderSwatches() {
  const wrap = el("color-swatches");
  wrap.innerHTML = "";

  // Current-color preview: a filled dot + label, so the active color is explicit
  // (not just inferred from which swatch has a ring).
  const current = document.createElement("span");
  current.className = "swatch-current";
  const cdot = document.createElement("span");
  cdot.className = "swatch-current-dot";
  cdot.style.background = effectiveColor();
  const clabel = document.createElement("span");
  clabel.textContent =
    selectedColor === null
      ? "Current: provider default"
      : `Current: ${PALETTE.find((c) => c.hex === selectedColor)?.name ?? "custom"}`;
  current.append(cdot, clabel);
  wrap.append(current);

  const row = document.createElement("div");
  row.className = "swatch-row";
  const make = (hex: string | null, title: string) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "swatch";
    b.title = title;
    if (hex) b.style.background = hex;
    else b.classList.add("swatch-default"); // diagonal-slash "no override" chip
    if (selectedColor === hex) b.classList.add("selected");
    b.addEventListener("click", () => {
      selectedColor = hex;
      renderSwatches();
    });
    return b;
  };
  row.append(make(null, "Default (provider color)"));
  for (const c of PALETTE) row.append(make(c.hex, c.name));
  wrap.append(row);
}

function show(id: string, visible: boolean) {
  el(id).hidden = !visible;
}

function metaFor(kind: string): ProviderMeta | undefined {
  return kinds.find((k) => k.kind === kind);
}

function currentMeta(): ProviderMeta | undefined {
  return metaFor(el<HTMLSelectElement>("kind").value);
}

function editingId(): string {
  return el<HTMLInputElement>("id").value;
}

// --- Result banner -----------------------------------------------------------

function clearResult() {
  show("result", false);
}

// --- Toast (transient confirmation for global actions) -----------------------

let toastTimer: number | undefined;

function toast(msg: string) {
  const t = el("toast");
  t.textContent = msg;
  t.hidden = false;
  void t.offsetWidth; // reflow so the transition runs
  t.classList.add("show");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    t.classList.remove("show");
    window.setTimeout(() => {
      t.hidden = true;
    }, 200);
  }, 2400);
}

function showResult(kind: "ok" | "err" | "info", msg: string) {
  const r = el("result");
  r.className = `result result-${kind}`;
  const icon = kind === "ok" ? "✓" : kind === "err" ? "✕" : "…";
  r.innerHTML = `<span class="result-icon"></span><span class="result-msg"></span>`;
  r.querySelector(".result-icon")!.textContent = icon;
  r.querySelector(".result-msg")!.textContent = msg;
  r.hidden = false;
}

// --- Field metadata / visibility --------------------------------------------

/// Reflect a provider kind's metadata into the form (visibility, help, links).
/// Does NOT overwrite label/base values — callers decide that.
function applyMeta(m: ProviderMeta) {
  el("kind-help").textContent = m.help;

  const docs = el<HTMLAnchorElement>("docs-link");
  if (m.docs_url) {
    docs.hidden = false;
    docs.onclick = (e) => {
      e.preventDefault();
      invoke("open_url", { url: m.docs_url });
    };
  } else {
    docs.hidden = true;
    docs.onclick = null;
  }

  const baseUsed = m.requires_base_url || m.default_base_url !== null;
  show("base-field", baseUsed);
  show("secret-field", m.requires_secret);

  // Optional provider-specific field (e.g. BetterStack team slug).
  if (m.extra_label) {
    show("extra-field", true);
    el("extra-label").textContent = m.extra_label;
    el<HTMLInputElement>("extra").placeholder = m.extra_placeholder;
    el("extra-help").textContent = m.extra_help;
  } else {
    show("extra-field", false);
  }
  show("base-req", m.requires_base_url);
  show("secret-req", m.requires_secret && !editingId());

  el<HTMLInputElement>("base_url").placeholder = m.base_url_placeholder;
  el("base-help").textContent = m.requires_base_url
    ? "Required for this provider."
    : "";

  // Make it obvious why the key field is blank when editing.
  el("secret-help").textContent = editingId()
    ? "Leave blank to keep the saved key."
    : m.secret_label
      ? `Your ${m.secret_label}.`
      : "";

  updateSaveState();
}

/// New provider selected: prefill name + base URL with this provider's defaults.
function onKindChange() {
  const m = currentMeta();
  if (!m) return;
  el<HTMLInputElement>("label").value = m.name;
  el<HTMLInputElement>("base_url").value = m.default_base_url ?? "";
  el<HTMLInputElement>("extra").value = "";
  editingHasSavedKey = false; // switching kind in the dropdown = a fresh entry
  syncSecretField();
  clearResult();
  applyMeta(m);
  renderSwatches(); // the "Default" preview tracks the newly selected kind
}

// --- Validation --------------------------------------------------------------

function formValues(): { config: ProviderConfig; secret: string } {
  const intervalRaw = el<HTMLInputElement>("interval_secs").value;
  return {
    config: {
      id: editingId(),
      kind: el<HTMLSelectElement>("kind").value,
      label: el<HTMLInputElement>("label").value.trim(),
      base_url: el<HTMLInputElement>("base_url").value.trim() || null,
      interval_secs: intervalRaw ? parseInt(intervalRaw, 10) : null,
      extra: el<HTMLInputElement>("extra").value.trim() || null,
      color: selectedColor,
      // scope is server-owned (upsert_provider ignores whatever we send here).
      scope: null,
      // mute-duration dropdown is wired in the frontend-actions PR; null for now.
      mute_default_secs: null,
    },
    secret: el<HTMLInputElement>("secret").value,
  };
}

/// Returns a problem string, or null if the form is ready to save.
function problem(): string | null {
  const { config, secret } = formValues();
  const m = metaFor(config.kind);
  if (!config.label) return "Give this provider a name.";
  if (m?.requires_base_url && !config.base_url) return "A Base URL is required.";
  if (m?.requires_secret && !secret && !config.id) return "An API key is required.";
  return null;
}

function updateSaveState() {
  el<HTMLButtonElement>("save").disabled = problem() !== null;
}

// --- Secret reveal -----------------------------------------------------------

/// Reset the API-key field back to masked. Called whenever the form is
/// repopulated so a revealed key from one provider doesn't carry over.
function hideSecret() {
  el<HTMLInputElement>("secret").type = "password";
  const toggle = el<HTMLButtonElement>("secret-toggle");
  toggle.textContent = "Show";
  toggle.setAttribute("aria-label", "Show key");
}

/// Keep the key field's placeholder and the Show button honest about the three
/// states: adding/no-key, editing-with-saved-key-and-empty, and typing a key.
/// Called on load, on provider change, and as the field is edited.
function syncSecretField() {
  const input = el<HTMLInputElement>("secret");
  const toggle = el<HTMLButtonElement>("secret-toggle");
  const savedAndEmpty = editingHasSavedKey && input.value === "";

  input.placeholder = savedAndEmpty
    ? "•••••••••• — saved (leave blank to keep)"
    : "Paste your key";

  // Nothing to reveal when a saved key is hidden behind an empty field.
  toggle.disabled = savedAndEmpty;
  toggle.title = savedAndEmpty
    ? "The saved key isn't shown here. Type a new key to replace it."
    : "";
  if (savedAndEmpty) hideSecret(); // re-mask in case it was revealed
}

// --- Add/edit form collapse --------------------------------------------------

/// Show the add/edit form and hide the collapsed "Add a provider" trigger.
function openForm() {
  show("form-group", true);
  show("add-trigger-group", false);
}

/// Collapse the form back to the trigger button.
function collapseForm() {
  show("form-group", false);
  show("add-trigger-group", true);
}

// --- Provider list -----------------------------------------------------------

async function fillForm(p: ProviderConfig) {
  openForm();
  el<HTMLInputElement>("id").value = p.id;
  el<HTMLSelectElement>("kind").value = p.kind; // programmatic — no change event
  el<HTMLInputElement>("label").value = p.label;
  el<HTMLInputElement>("base_url").value = p.base_url ?? "";
  el<HTMLInputElement>("interval_secs").value = p.interval_secs?.toString() ?? "";
  el<HTMLInputElement>("extra").value = p.extra ?? "";
  el<HTMLInputElement>("secret").value = "";
  selectedColor = p.color ?? null;
  renderSwatches();
  hideSecret();
  editingHasSavedKey = await invoke<boolean>("provider_has_secret", { id: p.id });
  syncSecretField();
  el("form-title").textContent = `Edit ${p.label}`;
  el<HTMLButtonElement>("cancel").hidden = false;
  clearResult();
  const m = metaFor(p.kind);
  if (m) applyMeta(m);
  el("provider-form").scrollIntoView({ behavior: "smooth", block: "start" });
}

function resetForm() {
  el<HTMLFormElement>("provider-form").reset();
  el<HTMLInputElement>("id").value = "";
  el("form-title").textContent = "Add a provider";
  el<HTMLButtonElement>("cancel").hidden = true;
  editingHasSavedKey = false;
  selectedColor = null;
  renderSwatches();
  hideSecret();
  clearResult();
  onKindChange();
  collapseForm();
}

/// Open a blank "Add a provider" form (from the collapsed trigger).
function startAdd() {
  el<HTMLFormElement>("provider-form").reset();
  el<HTMLInputElement>("id").value = "";
  el("form-title").textContent = "Add a provider";
  el<HTMLButtonElement>("cancel").hidden = false;
  editingHasSavedKey = false;
  selectedColor = null;
  renderSwatches();
  hideSecret();
  clearResult();
  onKindChange();
  openForm();
  el("provider-form").scrollIntoView({ behavior: "smooth", block: "start" });
}

async function loadKinds() {
  kinds = await invoke<ProviderMeta[]>("get_provider_kinds");
  const sel = el<HTMLSelectElement>("kind");
  sel.innerHTML = "";
  for (const m of kinds) {
    const opt = document.createElement("option");
    opt.value = m.kind;
    opt.textContent = m.name;
    sel.append(opt);
  }
  sel.addEventListener("change", onKindChange);
  onKindChange();
}

interface Browser {
  name: string;
  app: string;
}

/// Populate the "Open links in" dropdown with detected browsers and reflect the
/// saved choice; persist on change.
async function loadBrowsers() {
  const browsers = await invoke<Browser[]>("get_browsers");
  const current = await invoke<string>("get_browser_app");
  const sel = el<HTMLSelectElement>("browser");
  sel.innerHTML = "";
  for (const b of browsers) {
    const opt = document.createElement("option");
    opt.value = b.app; // "" = system default
    opt.textContent = b.name;
    sel.append(opt);
  }
  // If a previously-chosen browser is no longer installed, fall back visually to
  // the default (the backend already falls back at open time).
  sel.value = browsers.some((b) => b.app === current) ? current : "";
  sel.addEventListener("change", async () => {
    try {
      await invoke("set_browser_app", { value: sel.value });
      toast("Browser preference saved.");
    } catch (e) {
      showResult("err", `Couldn't save: ${e}`);
    }
  });
}

/// Test a saved provider straight from its list row, using the stored key (the
/// backend falls back to the keychain when the passed secret is empty). Gives
/// quick feedback without opening the edit form and scrolling to Test.
async function testRow(p: ProviderConfig, btn: HTMLButtonElement) {
  btn.disabled = true;
  // Spin the refresh icon in place while the test runs (button stays compact).
  btn.classList.add("spinning");
  try {
    const res = await invoke<{ count: number; note: string | null }>("test_provider", {
      config: p,
      secret: "",
    });
    const msg = `“${p.label}”: ${res.count} monitor${res.count === 1 ? "" : "s"}`;
    if (res.note) toast(`${msg} — ${res.note}`);
    else toast(`${msg} ✓`);
  } catch (e) {
    toast(`“${p.label}” failed: ${e}`);
  } finally {
    btn.disabled = false;
    btn.classList.remove("spinning");
  }
}

async function loadProviders() {
  const providers = await invoke<ProviderConfig[]>("get_providers");
  const list = el<HTMLUListElement>("providers");
  list.innerHTML = "";

  if (providers.length === 0) {
    const li = document.createElement("li");
    li.className = "empty-row";
    li.textContent = "Nothing connected yet — add a provider below.";
    list.append(li);
    return;
  }

  for (const p of providers) {
    const hasSecret = await invoke<boolean>("provider_has_secret", { id: p.id });
    const kindName = metaFor(p.kind)?.name ?? p.kind;

    const li = document.createElement("li");
    li.className = "provider-row";

    const dot = document.createElement("span");
    dot.className = hasSecret ? "provider-dot" : "provider-dot warn";

    const info = document.createElement("div");
    info.className = "provider-info";
    const name = document.createElement("div");
    name.className = "provider-name";
    name.textContent = p.label;
    const sub = document.createElement("div");
    sub.className = "provider-sub";
    sub.textContent = hasSecret ? kindName : `${kindName} · no key set`;
    if (!hasSecret) sub.classList.add("warn");
    info.append(name, sub);

    // Inline two-step confirm (no native dialogs in this webview).
    const btns = document.createElement("div");
    btns.className = "provider-actions";
    let revertTimer: number | undefined;

    function mkBtn(text: string, cls: string): HTMLButtonElement {
      const b = document.createElement("button");
      b.type = "button";
      b.className = `btn btn-sm ${cls}`.trim();
      b.textContent = text;
      return b;
    }

    // Icon-only action button with a hover tooltip (the word). The SVG carries no
    // text, so the title + aria-label keep it labeled/accessible.
    function mkIcon(svg: string, title: string, cls: string): HTMLButtonElement {
      const b = document.createElement("button");
      b.type = "button";
      b.className = `btn btn-icon btn-sm ${cls}`.trim();
      b.innerHTML = svg;
      b.title = title;
      b.setAttribute("aria-label", title);
      return b;
    }

    function showDefault() {
      if (revertTimer) clearTimeout(revertTimer);
      btns.innerHTML = "";
      const test = mkIcon(ICONS.test, "Test", "");
      test.addEventListener("click", () => testRow(p, test));
      const edit = mkIcon(ICONS.edit, "Edit", "");
      edit.addEventListener("click", () => fillForm(p));
      const del = mkIcon(ICONS.remove, "Remove", "btn-icon-danger");
      del.addEventListener("click", showConfirm);
      btns.append(test, edit, del);
    }

    function showConfirm() {
      btns.innerHTML = "";
      const lbl = document.createElement("span");
      lbl.className = "confirm-label";
      lbl.textContent = "Remove?";
      const cancel = mkBtn("Cancel", "");
      cancel.addEventListener("click", showDefault);
      const confirmBtn = mkBtn("Remove", "btn-danger");
      confirmBtn.addEventListener("click", async () => {
        if (revertTimer) clearTimeout(revertTimer);
        try {
          await invoke("delete_provider", { id: p.id });
          if (editingId() === p.id) resetForm();
          await loadProviders();
          toast(`Removed “${p.label}”.`);
        } catch (e) {
          showResult("err", `Couldn't remove: ${e}`);
        }
      });
      btns.append(lbl, cancel, confirmBtn);
      // Auto-revert if the user walks away, so a row never stays "armed".
      revertTimer = window.setTimeout(showDefault, 4000);
    }

    showDefault();
    li.append(dot, info, btns);
    list.append(li);
  }
}

// --- Wire up -----------------------------------------------------------------

window.addEventListener("DOMContentLoaded", async () => {
  await loadKinds();
  await loadProviders();
  await loadBrowsers();
  renderSwatches();

  // Live validation + clear stale results as the user types.
  for (const id of ["label", "base_url", "secret", "interval_secs"]) {
    el(id).addEventListener("input", () => {
      clearResult();
      updateSaveState();
    });
  }
  // Typing a key (or clearing it) flips the placeholder/Show-button state.
  el("secret").addEventListener("input", syncSecretField);

  el("test").addEventListener("click", async () => {
    const { config, secret } = formValues();
    const p = problem();
    // Allow testing an existing provider with the saved key even if blank.
    if (p && !(config.id && p.includes("API key"))) return showResult("err", p);
    showResult("info", "Testing connection…");
    try {
      const res = await invoke<{ count: number; note: string | null }>("test_provider", {
        config,
        secret,
      });
      const base = `Connected — found ${res.count} monitor${res.count === 1 ? "" : "s"}.`;
      // A note (e.g. Healthchecks read-only key) is advisory, not an error.
      if (res.note) showResult("info", `${base} ${res.note}`);
      else showResult("ok", base);
    } catch (e) {
      showResult("err", `${e}`);
    }
  });

  el("provider-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const { config, secret } = formValues();
    const p = problem();
    if (p) return showResult("err", p);
    const wasEditing = !!config.id;
    const savedLabel = config.label;
    try {
      await invoke("upsert_provider", { config, secret: secret || null });
      await loadProviders();
      resetForm();
      toast(`${wasEditing ? "Updated" : "Added"} “${savedLabel}”.`);
    } catch (e) {
      showResult("err", `Couldn't save: ${e}`);
    }
  });

  el("cancel").addEventListener("click", resetForm);
  el("add-trigger").addEventListener("click", startAdd);

  // Reveal toggle so a pasted key can be verified before saving. (Disabled when
  // a saved key is hidden behind an empty field — there's nothing to reveal.)
  el<HTMLButtonElement>("secret-toggle").addEventListener("click", () => {
    const input = el<HTMLInputElement>("secret");
    const toggle = el<HTMLButtonElement>("secret-toggle");
    if (toggle.disabled) return;
    const reveal = input.type === "password";
    input.type = reveal ? "text" : "password";
    toggle.textContent = reveal ? "Hide" : "Show";
    toggle.setAttribute("aria-label", reveal ? "Hide key" : "Show key");
  });
});
