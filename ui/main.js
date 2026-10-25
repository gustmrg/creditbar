const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const BEIJING = "Asia/Shanghai";
const HOLIDAYS = {
  元旦: "New Year’s Day",
  春节: "Chinese New Year",
  清明: "Qingming Festival",
  劳动节: "Labor Day",
  端午节: "Dragon Boat Festival",
  中秋节: "Mid-Autumn Festival",
  国庆节: "National Day",
};

let snapshot = null;
let reloading = false;
/** Providers whose API key editor is open. */
const editing = new Set();
let lastBalances = [];

const $ = (id) => document.getElementById(id);

/** Builds an element; strings become text nodes, so nothing is parsed as HTML. */
function el(tag, props = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter((c) => c != null && c !== false));
  return node;
}

const beijingTime = (iso) =>
  new Date(iso).toLocaleTimeString([], { timeZone: BEIJING, hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

const localTime = (iso) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

const periodName = (period) => (period === "peak" ? "Peak hours" : "Off-peak hours");

async function reload() {
  if (reloading) return;
  reloading = true;
  try {
    const state = await invoke("get_state");
    snapshot = state.snapshot;
    renderSnapshot();
    renderBalances(state.balances);
  } finally {
    reloading = false;
  }
}

function renderSnapshot() {
  const peak = snapshot.period === "peak";
  document.body.dataset.period = snapshot.period;
  $("period").textContent = periodName(snapshot.period);
  $("price").textContent = peak ? "Full price (100%)" : "Half the peak price (50%)";
  $("multiplier").textContent = peak ? "×1.0" : "×0.5";

  const sameZone = new Date().getTimezoneOffset() === -480;
  const at = beijingTime(snapshot.nextTransition);
  $("next").textContent =
    `${periodName(snapshot.nextPeriod)} at ${at}` +
    (sameZone ? "" : ` · ${localTime(snapshot.nextTransition)} your time`);
  $("day").textContent = dayText(snapshot.dayInfo);
  tick();
}

function dayText({ kind, scheduleIsCovered }) {
  const holiday = HOLIDAYS[kind.name] ?? "Public holiday";
  const text = {
    publicHoliday: `${holiday} · off-peak all day`,
    alternateWorkdayWeekend: `${holiday} make-up workday on a weekend · off-peak all day`,
    weekend: "Weekend · off-peak all day",
    regularWeekday: "Weekday · peak 09:00–12:00 and 14:00–18:00",
  }[kind.type];
  return scheduleIsCovered ? text : `${text} · holiday calendar not available for this year`;
}

function tick() {
  if (!snapshot) return;
  const now = Date.now();
  const next = Date.parse(snapshot.nextTransition);
  const start = Date.parse(snapshot.intervalStart);
  const seconds = Math.max(0, Math.floor((next - now) / 1000));
  const pad = (n) => String(n).padStart(2, "0");
  $("countdown").textContent =
    `${pad(Math.floor(seconds / 3600))}:${pad(Math.floor((seconds % 3600) / 60))}:${pad(seconds % 60)}`;
  $("progress").style.width = `${Math.min(100, Math.max(0, ((now - start) / (next - start)) * 100))}%`;
  if (seconds === 0) reload();
}

function formatAmount(currency, value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return `${value} ${currency}`;
  try {
    return new Intl.NumberFormat([], { style: "currency", currency }).format(number);
  } catch {
    return `${value} ${currency}`;
  }
}

function renderBalances(balances) {
  lastBalances = balances;
  $("balances").replaceChildren(el("div", { className: "label" }, "Balance"), ...balances.map(renderProvider));
}

function renderProvider(view) {
  const id = view.providerId;
  const isEditing = editing.has(id) || !view.hasKey;

  const refresh = view.hasKey
    ? el("button", {
        textContent: view.refreshing ? "Refreshing…" : "Refresh",
        disabled: view.refreshing,
        onclick: () => invoke("refresh_balance", { providerId: id }),
      })
    : null;
  const updated = view.lastRefreshed
    ? el("span", { className: "muted small" }, localTime(view.lastRefreshed))
    : null;

  const amounts = (view.balance?.amounts ?? []).map((a) =>
    el(
      "div",
      {},
      el("div", { className: "amount" }, formatAmount(a.currency, a.total)),
      a.granted != null &&
        el(
          "div",
          { className: "muted small" },
          `Granted ${formatAmount(a.currency, a.granted)} · topped up ${formatAmount(a.currency, a.toppedUp)}`,
        ),
    ),
  );

  return el(
    "div",
    { className: "provider" },
    el("div", { className: "provider-head" }, el("span", { className: "name" }, view.displayName), updated, refresh),
    ...amounts,
    view.balance && !view.balance.isAvailable && el("p", { className: "error small" }, "Not enough balance for API calls."),
    view.error && el("p", { className: "error small" }, view.error),
    !view.hasKey && el("p", { className: "muted small" }, "No API key saved, so no balance is requested."),
    isEditing ? keyEditor(view) : el("button", { className: "link small", onclick: () => edit(id, true) },
      view.suggestsReplacingKey ? "Replace API key" : "Change API key"),
  );
}

function keyEditor(view) {
  const id = view.providerId;
  const input = el("input", { type: "password", placeholder: "sk-…", autocomplete: "off", spellcheck: false });
  const message = el("p", { className: "error small" });
  const save = async () => {
    try {
      await invoke("save_api_key", { providerId: id, key: input.value });
      editing.delete(id);
    } catch (error) {
      message.textContent = String(error);
    }
  };
  input.addEventListener("keydown", (e) => e.key === "Enter" && save());

  return el(
    "div",
    {},
    el(
      "div",
      { className: "row" },
      input,
      el("button", { className: "primary", textContent: "Save", onclick: save }),
    ),
    view.hasKey &&
      el(
        "div",
        { className: "row" },
        el("button", { className: "link small", textContent: "Cancel", onclick: () => edit(id, false) }),
        el("button", {
          className: "link small error",
          textContent: "Remove key",
          onclick: () => invoke("remove_api_key", { providerId: id }).then(() => editing.delete(id)),
        }),
      ),
    message,
  );
}

function edit(id, open) {
  open ? editing.add(id) : editing.delete(id);
  renderBalances(lastBalances);
}

listen("snapshot", (e) => {
  snapshot = e.payload;
  renderSnapshot();
});
listen("balances", (e) => renderBalances(e.payload));
listen("panel-shown", reload);

$("quit").addEventListener("click", () => invoke("quit"));
document.addEventListener("keydown", (e) => e.key === "Escape" && invoke("hide_panel"));

setInterval(tick, 1000);
reload();
