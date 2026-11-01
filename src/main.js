const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;
const { LogicalSize } = window.__TAURI__.dpi;

const $ = (id) => document.getElementById(id);
const WIDTH = 340;
let current = null;

const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

function fmtDuration(ms) {
  if (ms <= 0) return "now";
  const mins = Math.round(ms / 60000);
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;
  if (d > 0) return h ? `${d}d ${h}h` : `${d}d`;
  if (h > 0) return m ? `${h}h ${m}m` : `${h}h`;
  return `${m}m`;
}

function fmtClock(date, withDay) {
  const opts = { hour: "2-digit", minute: "2-digit" };
  if (withDay) opts.weekday = "short";
  return date.toLocaleString(undefined, opts);
}

function fmtAgo(iso) {
  const s = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  return `${Math.round(s / 3600)}h ago`;
}

function severity(limit) {
  const status = limit.pace?.status;
  if (limit.used_percent >= 95 || status === "exhausted") return "bad";
  if (limit.used_percent >= 75 || status === "runs_out") return "warn";
  return "ok";
}

function paceText(limit) {
  const p = limit.pace;
  if (!p) return "";
  if (limit.used_percent <= 0) return `<span class="muted">Not used yet</span>`;
  const resetsAt = limit.resets_at ? new Date(limit.resets_at) : null;
  const delta = Math.round(p.delta_percent);
  const deltaText =
    Math.abs(delta) < 2
      ? "On pace"
      : delta > 0
        ? `<b>${delta}%</b> ahead of pace`
        : `<b>${-delta}%</b> under pace`;

  switch (p.status) {
    case "exhausted":
      return `<span class="bad">Limit reached${resetsAt ? ` · back in ${fmtDuration(resetsAt - Date.now())}` : ""}</span>`;
    case "too_early":
      return `<span class="muted">Too early to project</span>`;
    case "runs_out": {
      const out = new Date(p.runs_out_at);
      const cls = limit.window_hours > 5 ? "bad" : "warn";
      return `<span class="${cls}">${deltaText} · runs out in ${fmtDuration(out - Date.now())}</span>`;
    }
    default: {
      const proj = p.projected_percent != null ? ` · ~${Math.round(p.projected_percent)}% at reset` : "";
      return `<span class="ok">${deltaText} · lasts to reset</span><span class="muted">${proj}</span>`;
    }
  }
}

function card(limit) {
  const used = Math.max(0, Math.min(100, limit.used_percent));
  const left = Math.max(0, 100 - limit.used_percent);
  const sev = severity(limit);
  const resetsAt = limit.resets_at ? new Date(limit.resets_at) : null;
  const resetText = resetsAt
    ? `Resets in ${fmtDuration(resetsAt - Date.now())} · ${fmtClock(resetsAt, limit.window_hours > 24)}`
    : "";
  const tick = limit.pace
    ? `<div class="tick" style="left:${limit.pace.expected_percent.toFixed(2)}%" title="Even pace: ${Math.round(limit.pace.expected_percent)}%"></div>`
    : "";

  return `
    <div class="card">
      <div class="row">
        <span class="label">${esc(limit.label)}</span>
        <span class="muted">${resetText}</span>
      </div>
      <div class="big">${Math.round(left)}% left<small>${Math.round(used)}% used</small></div>
      <div class="bar"><div class="fill ${sev}" style="width:${used}%"></div>${tick}</div>
      <div class="pace">${paceText(limit)}</div>
    </div>`;
}

function render(state) {
  current = state;
  const snap = state?.snapshot;

  $("refresh").classList.toggle("spinning", !!state?.refreshing);
  $("plan").hidden = !snap?.plan;
  $("plan").textContent = snap?.plan ?? "";
  $("updated").textContent = snap ? fmtAgo(snap.fetched_at) : "";

  $("error").hidden = !state?.error;
  $("error").textContent = state?.error ?? "";

  if (snap && snap.limits.length) {
    $("limits").innerHTML = snap.limits.map(card).join("");
  } else if (!state?.error) {
    $("limits").innerHTML = `<div class="empty">Loading usage…</div>`;
  } else {
    $("limits").innerHTML = "";
  }
  fitWindow();
}

function fitWindow() {
  const h = Math.ceil(document.getElementById("panel").getBoundingClientRect().height);
  getCurrentWindow().setSize(new LogicalSize(WIDTH, h)).catch(() => {});
}

$("refresh").addEventListener("click", async () => {
  render({ ...current, refreshing: true });
  render(await invoke("refresh_now"));
});
$("quit").addEventListener("click", () => invoke("quit"));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") getCurrentWindow().hide();
});

listen("state", (e) => render(e.payload));
invoke("get_state").then(render);
// Keep "resets in" and "updated" texts fresh while the popup is open.
setInterval(() => current && render(current), 30_000);
