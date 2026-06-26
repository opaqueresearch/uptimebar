import { invoke } from "@tauri-apps/api/core";

interface ProviderConfig {
  id: string;
  kind: string;
  label: string;
  base_url: string | null;
  interval_secs: number | null;
  extra: string | null;
}

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

function el<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
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

// --- Provider list -----------------------------------------------------------

async function fillForm(p: ProviderConfig) {
  el<HTMLInputElement>("id").value = p.id;
  el<HTMLSelectElement>("kind").value = p.kind; // programmatic — no change event
  el<HTMLInputElement>("label").value = p.label;
  el<HTMLInputElement>("base_url").value = p.base_url ?? "";
  el<HTMLInputElement>("interval_secs").value = p.interval_secs?.toString() ?? "";
  el<HTMLInputElement>("extra").value = p.extra ?? "";
  el<HTMLInputElement>("secret").value = "";
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
  hideSecret();
  clearResult();
  onKindChange();
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

    function showDefault() {
      if (revertTimer) clearTimeout(revertTimer);
      btns.innerHTML = "";
      const edit = mkBtn("Edit", "");
      edit.addEventListener("click", () => fillForm(p));
      const del = mkBtn("Remove", "");
      del.addEventListener("click", showConfirm);
      btns.append(edit, del);
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
