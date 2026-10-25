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

/** What a balance figure measures (see `AmountKind` in the core). */
const AMOUNT_KINDS = {
  accountBalance: "Account balance",
  keyLimitRemaining: "Left on this key’s limit",
  keySpent: "Spent with this key (no limit set)",
};

let snapshot = null;
let reloading = false;
/** Providers whose API key editor is open. */
const editing = new Set();
let lastBalances = [];
/** Calendar view and the Beijing date (`YYYY-MM-DD`) it is centred on. */
let calendarView = "month";
let calendarCursor = null;

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
    await loadCalendar();
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
  const isEditing = editing.has(id);

  // Without a key the header only offers to add one; the editor opens on demand.
  const action = view.hasKey
    ? el("button", {
        textContent: view.refreshing ? "Refreshing…" : "Refresh",
        disabled: view.refreshing,
        onclick: () => invoke("refresh_balance", { providerId: id }),
      })
    : !isEditing && el("button", { className: "link small", textContent: "Add API key", onclick: () => edit(id, true) });
  const updated = view.lastRefreshed
    ? el("span", { className: "muted small" }, localTime(view.lastRefreshed))
    : null;

  const amounts = (view.balance?.amounts ?? []).map((a) =>
    el(
      "div",
      {},
      el("div", { className: "amount" }, formatAmount(a.currency, a.total)),
      el(
        "div",
        { className: "muted small" },
        [
          AMOUNT_KINDS[a.kind],
          a.granted != null && `promo ${formatAmount(a.currency, a.granted)}`,
          a.toppedUp != null && `paid ${formatAmount(a.currency, a.toppedUp)}`,
        ]
          .filter(Boolean)
          .join(" · "),
      ),
    ),
  );

  return el(
    "div",
    { className: "provider" },
    el("div", { className: "provider-head" }, el("span", { className: "name" }, view.displayName), updated, action),
    ...amounts,
    view.balance && !view.balance.isAvailable && el("p", { className: "error small" }, "Not enough balance for API calls."),
    view.error && el("p", { className: "error small" }, view.error),
    isEditing
      ? keyEditor(view)
      : view.hasKey &&
          el("button", { className: "link small", onclick: () => edit(id, true) },
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
  queueMicrotask(() => input.focus());

  return el(
    "div",
    {},
    el(
      "div",
      { className: "row" },
      input,
      el("button", { className: "primary", textContent: "Save", onclick: save }),
    ),
    el(
      "div",
      { className: "row" },
      el("button", { className: "link small", textContent: "Cancel", onclick: () => edit(id, false) }),
      view.hasKey &&
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

// Calendar

/** Today's date in Beijing as `YYYY-MM-DD`. */
const beijingToday = () => new Intl.DateTimeFormat("en-CA", { timeZone: BEIJING }).format(new Date());

/** Dates are handled as UTC midnights so the local time zone never shifts them. */
const parseDay = (day) => new Date(`${day}T00:00:00Z`);
const formatDay = (date) => date.toISOString().slice(0, 10);

function moveCursor(step) {
  const date = parseDay(calendarCursor);
  if (calendarView === "month") {
    date.setUTCDate(1);
    date.setUTCMonth(date.getUTCMonth() + step);
  } else {
    date.setUTCDate(date.getUTCDate() + 7 * step);
  }
  calendarCursor = formatDay(date);
  loadCalendar();
}

async function loadCalendar() {
  calendarCursor ??= beijingToday();
  const date = parseDay(calendarCursor);
  $("legend-off").textContent = calendarView === "month" ? "Off-peak all day" : "Off-peak";
  $("legend-holiday").hidden = calendarView !== "month";
  if (calendarView === "month") {
    const days = await invoke("month_calendar", { year: date.getUTCFullYear(), month: date.getUTCMonth() + 1 });
    $("cal-title").textContent = date.toLocaleDateString([], { month: "long", year: "numeric", timeZone: "UTC" });
    $("calendar").replaceChildren(renderMonth(days));
  } else {
    const days = await invoke("week_grid", { date: calendarCursor });
    const short = (day) => parseDay(day).toLocaleDateString([], { month: "short", day: "numeric", timeZone: "UTC" });
    $("cal-title").textContent = `${short(days[0].date)} – ${short(days[6].date)}`;
    $("calendar").replaceChildren(renderWeek(days));
  }
}

const weekdayName = (day, style) => parseDay(day).toLocaleDateString([], { weekday: style, timeZone: "UTC" });

function renderMonth(days) {
  const grid = el("div", { className: "month" });
  // 2024-01-01 was a Monday.
  for (let i = 0; i < 7; i++) {
    grid.append(el("div", { className: "head" }, weekdayName(`2024-01-0${i + 1}`, "narrow")));
  }
  const leading = (parseDay(days[0].date).getUTCDay() + 6) % 7;
  for (let i = 0; i < leading; i++) grid.append(el("div"));

  const today = beijingToday();
  for (const day of days) {
    const type = day.info.kind.type;
    const classes = ["day"];
    if (type !== "regularWeekday") classes.push("off");
    if (type === "publicHoliday") classes.push("holiday");
    if (day.date === today) classes.push("today");
    grid.append(
      el("div", { className: classes.join(" "), title: `${day.date} · ${dayText(day.info)}` }, String(Number(day.date.slice(8)))),
    );
  }
  return grid;
}

function renderWeek(days) {
  const grid = el("div", { className: "week" });
  grid.append(el("span"));
  for (let hour = 0; hour < 24; hour++) {
    grid.append(el("span", { className: "hour-label" }, hour % 6 === 0 ? String(hour) : ""));
  }

  const today = beijingToday();
  const currentHour = Number(
    new Intl.DateTimeFormat("en-GB", { timeZone: BEIJING, hour: "2-digit", hourCycle: "h23" }).format(new Date()),
  );
  for (const day of days) {
    const isToday = day.date === today;
    grid.append(
      el(
        "span",
        { className: `label-cell${isToday ? " today" : ""}`, title: dayText(day.info) },
        `${weekdayName(day.date, "short")} ${Number(day.date.slice(8))}`,
      ),
    );
    day.hours.forEach((period, hour) => {
      const classes = ["cell"];
      if (period === "peak") classes.push("peak");
      if (isToday && hour === currentHour) classes.push("now");
      const until = String(hour + 1).padStart(2, "0");
      grid.append(el("span", { className: classes.join(" "), title: `${String(hour).padStart(2, "0")}:00–${until}:00 · ${periodName(period)}` }));
    });
  }
  return grid;
}

for (const tab of document.querySelectorAll("[role=tab]")) {
  tab.addEventListener("click", () => {
    calendarView = tab.dataset.view;
    for (const other of document.querySelectorAll("[role=tab]")) {
      other.setAttribute("aria-selected", String(other === tab));
    }
    loadCalendar();
  });
}
$("cal-prev").addEventListener("click", () => moveCursor(-1));
$("cal-next").addEventListener("click", () => moveCursor(1));

listen("snapshot", (e) => {
  snapshot = e.payload;
  renderSnapshot();
  loadCalendar();
});
listen("balances", (e) => renderBalances(e.payload));
listen("panel-shown", () => {
  // Opening the panel goes back to today.
  calendarCursor = null;
  reload();
});

$("quit").addEventListener("click", () => invoke("quit"));
document.addEventListener("keydown", (e) => e.key === "Escape" && invoke("hide_panel"));

setInterval(tick, 1000);
reload();
