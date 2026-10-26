const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const BEIJING = "Asia/Shanghai";

/** Filled from the backend's `get_i18n` before the first render. */
let i18n = { locale: "en-US", strings: {}, holidayKeys: {} };

/** A UI string, with `{name}` placeholders replaced from `vars`. */
function t(key, vars = {}) {
  const text = i18n.strings[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? vars[name] : match));
}

function applyStaticStrings() {
  document.documentElement.lang = i18n.locale;
  for (const node of document.querySelectorAll("[data-i18n]")) node.textContent = t(node.dataset.i18n);
  for (const node of document.querySelectorAll("[data-i18n-label]")) {
    node.setAttribute("aria-label", t(node.dataset.i18nLabel));
  }
}

let snapshot = null;
let settings = { showsDeepseekPricing: true };
let providers = [];
let reloading = false;
/** Providers whose API key editor is open. */
const editing = new Set();
/** The provider whose card is open, or null. Remembered per viewer. */
let expanded = loadExpanded();
/** Calendar view and the date (`YYYY-MM-DD`) it is centred on; null means today. */
let calendarView = "month";
let calendarCursor = null;

const DEEPSEEK = "deepseek";
/** A quota this far used is drawn as a warning (matches the tray's threshold). */
const WARN_PERCENT = 90;

const $ = (id) => document.getElementById(id);

/** Builds an element; strings become text nodes, so nothing is parsed as HTML. */
function el(tag, props = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.flat(Infinity).filter((c) => c != null && c !== false));
  return node;
}

function loadExpanded() {
  try {
    return localStorage.getItem("expanded");
  } catch {
    return null;
  }
}

function saveExpanded() {
  try {
    if (expanded) localStorage.setItem("expanded", expanded);
    else localStorage.removeItem("expanded");
  } catch {
    // Storage may be unavailable; the choice just won't survive a restart.
  }
}

const beijingTime = (iso) =>
  new Date(iso).toLocaleTimeString(i18n.locale, {
    timeZone: BEIJING,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });

const localTime = (iso) => new Date(iso).toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });

const periodName = (period) => t(period === "peak" ? "period.peak" : "period.offPeak");
const periodShort = (period) => t(period === "peak" ? "period.peakShort" : "period.offPeakShort");

const pad = (n) => String(n).padStart(2, "0");
const clock = (seconds) => `${pad(Math.floor(seconds / 3600))}:${pad(Math.floor((seconds % 3600) / 60))}:${pad(seconds % 60)}`;

/** "45m", "5h 20m", "3d 4h". */
function shortDuration(ms) {
  const minutes = Math.max(1, Math.ceil(ms / 60000));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${pad(minutes % 60)}m`;
  return `${minutes}m`;
}

async function reload() {
  if (reloading) return;
  reloading = true;
  try {
    const state = await invoke("get_state");
    snapshot = state.snapshot;
    settings = state.settings;
    renderSettings();
    renderSnapshot();
    renderProviders(state.providers);
    await loadCalendar();
  } finally {
    reloading = false;
  }
}

// DeepSeek pricing

function renderSnapshot() {
  const peak = snapshot.period === "peak";
  document.body.dataset.period = snapshot.period;
  Aquarium.setPeriod(snapshot.period);
  $("period").textContent = periodName(snapshot.period);
  $("price").textContent = t(peak ? "price.peak" : "price.offPeak");
  $("multiplier").textContent = peak ? "×1.0" : "×0.5";

  const sameZone = new Date().getTimezoneOffset() === -480;
  const at = beijingTime(snapshot.nextTransition);
  const next = t("panel.nextAt", { period: periodName(snapshot.nextPeriod), time: at });
  $("next").textContent = sameZone
    ? next
    : `${next} · ${t("panel.yourTime", { time: localTime(snapshot.nextTransition) })}`;
  $("day").textContent = dayText(snapshot.dayInfo);
  tick();
}

function dayText({ kind, scheduleIsCovered }) {
  const name = t(`holiday.${i18n.holidayKeys[kind.name] ?? "public"}`);
  const text = t(
    {
      publicHoliday: "day.holiday",
      alternateWorkdayWeekend: "day.makeup",
      weekend: "day.weekend",
      regularWeekday: "day.weekday",
    }[kind.type],
    { name },
  );
  return scheduleIsCovered ? text : t("day.notCovered", { text });
}

/** "Off-peak ×0.5 · 03:12:44" for the DeepSeek row. */
function pricingBadge(seconds) {
  const multiplier = snapshot.period === "peak" ? "×1.0" : "×0.5";
  return `${periodShort(snapshot.period)} ${multiplier} · ${clock(seconds)}`;
}

function tick() {
  const now = Date.now();
  tickPeaks(now);
  if (!snapshot) return;
  const next = Date.parse(snapshot.nextTransition);
  const start = Date.parse(snapshot.intervalStart);
  const seconds = Math.max(0, Math.floor((next - now) / 1000));
  $("countdown").textContent = clock(seconds);
  $("progress").style.width = `${Math.min(100, Math.max(0, ((now - start) / (next - start)) * 100))}%`;
  const badge = $("pricing-badge");
  if (badge) badge.textContent = pricingBadge(seconds);
  if (seconds === 0) reload();
}

// Providers

function formatMoney(money) {
  const number = Number(money.amount);
  if (!Number.isFinite(number)) return `${money.amount} ${money.currency}`;
  try {
    return new Intl.NumberFormat(i18n.locale, { style: "currency", currency: money.currency }).format(number);
  } catch {
    return `${money.amount} ${money.currency}`;
  }
}

const formatCount = (count) => new Intl.NumberFormat(i18n.locale).format(count);

/** Money, tokens, credits or calls. */
function formatQuantity(quantity) {
  if (quantity.unit === "money") return formatMoney(quantity.value);
  return t(`quantity.${quantity.unit}`, { amount: formatCount(quantity.value) });
}

/** "$25.50 of $100.00", "224 of 1,000 calls": counts name their unit once. */
const usedOf = (window) =>
  t("window.usedOf", {
    used: window.used.unit === "money" ? formatMoney(window.used.value) : formatCount(window.used.value),
    limit: formatQuantity(window.limit),
  });

const isMoney = (window) => window.used?.unit === "money" && window.limit?.unit === "money";

const remaining = (window) => ({
  currency: window.limit.value.currency,
  amount: String(Math.max(0, Number(window.limit.value.amount) - Number(window.used.value.amount))),
});

const showsPricing = (view) => view.info.id === DEEPSEEK && settings.showsDeepseekPricing;

/** Rows: providers with a key, plus DeepSeek while its pricing is shown. */
const inOverview = (view) => view.hasKey || showsPricing(view);

const needsAttention = (view) =>
  view.hasKey &&
  (view.error != null ||
    (view.snapshot != null &&
      (!view.snapshot.canMakeCalls || view.snapshot.windows.some((w) => w.usedPercent >= WARN_PERCENT))));

function noteText(note) {
  return t(`note.${note.kind}`, note.amount ? { amount: formatMoney(note.amount) } : {});
}

const isWarning = (note) => note.kind === "cannotMakeCalls" || note.kind === "deficit";

function windowTitle(window) {
  const title = t(`window.${window.kind}`);
  return window.reset ? `${title} · ${t(`window.period.${window.reset}`)}` : title;
}

function windowStatus(window) {
  const used = t("window.percentUsed", { percent: Math.round(window.usedPercent) });
  if (!window.resetsAt) return used;
  const duration = shortDuration(Date.parse(window.resetsAt) - Date.now());
  return `${used} · ${t("window.resetsIn", { duration })}`;
}

function meter(percent) {
  const fill = el("div", { className: percent >= WARN_PERCENT ? "warn" : "" });
  fill.style.width = `${Math.min(100, Math.max(0, percent))}%`;
  return el("div", { className: "meter" }, fill);
}

/** The headline figure: account balance, else what's left of a quota, else spend. */
function headline(snap) {
  if (!snap) return "—";
  if (snap.balances.length) return formatMoney(snap.balances[0].total);
  const [first] = snap.windows;
  if (first) {
    if (isMoney(first)) return formatMoney(remaining(first));
    return t("window.percentLeft", { percent: Math.max(0, Math.round(100 - first.usedPercent)) });
  }
  const spent = snap.details.find((d) => d.kind === "spentWithKey");
  return spent ? formatMoney(spent.value) : "—";
}

function partsText(balance) {
  return balance.parts.map(([part, money]) => `${t(`part.${part}`)} ${formatMoney(money)}`).join(" · ");
}

/** The single line under a row's name. */
function summary(view) {
  if (view.error) return el("span", { className: "error" }, view.error);
  if (showsPricing(view)) return el("span", { id: "pricing-badge" }, snapshot ? pricingBadge(0) : "");
  const snap = view.snapshot;
  if (!snap) return view.refreshing ? t("balance.refreshing") : "";
  const warning = snap.notes.find(isWarning);
  if (warning) return el("span", { className: "error" }, noteText(warning));
  if (snap.windows.length) return `${windowTitle(snap.windows[0])} · ${windowStatus(snap.windows[0])}`;
  if (snap.balances.length && snap.balances[0].parts.length) return partsText(snap.balances[0]);
  return snap.notes.length ? noteText(snap.notes[0]) : "";
}

function renderProviders(views) {
  providers = views;
  const rows = views.filter(inOverview);
  if (!rows.some((v) => v.info.id === expanded)) expanded = rows.length === 1 ? rows[0].info.id : null;

  // Park the pricing section before the cards that may hold it are replaced.
  $("parking").append($("pricing"));

  $("overview").hidden = rows.length === 0;
  $("overview").replaceChildren(...rows.map(renderRow));
  renderAddProviders(views.filter((v) => !inOverview(v)), rows.length === 0);

  const slot = $("pricing-slot");
  if (slot) slot.append($("pricing"));
  tick();
  Aquarium.wake();
}

/** The provider's logo (`ui/icons/<id>.svg`, from CodexBar) in the brand's colour, or the text
 * colour for black-and-white logos; an amber dot when it needs attention. */
function providerIcon(info, attention = false) {
  const logo = el("i", { className: "logo" });
  if (info.logoColor) logo.style.background = info.logoColor;
  logo.style.setProperty("--logo", `url("icons/${info.id}.svg")`);
  return el("span", { className: `provider-icon${attention ? " attention" : ""}`, ariaHidden: "true" }, logo);
}

function renderRow(view) {
  const id = view.info.id;
  const open = expanded === id;
  const firstWindow = view.snapshot?.windows[0];

  const head = el(
    "button",
    {
      className: "provider-row",
      ariaExpanded: String(open),
      onclick: () => {
        expanded = open ? null : id;
        saveExpanded();
        renderProviders(providers);
      },
    },
    providerIcon(view.info, needsAttention(view)),
    el(
      "span",
      { className: "row-text" },
      el("span", { className: "row-name" }, view.info.name),
      el("span", { className: "row-summary muted small" }, summary(view)),
      firstWindow && meter(firstWindow.usedPercent),
    ),
    el("span", { className: "row-value" }, headline(view.snapshot)),
    el("span", { className: "chevron", ariaHidden: "true" }, open ? "▾" : "▸"),
  );
  return el("div", { className: "provider" }, head, open && renderCard(view));
}

function renderCard(view) {
  const id = view.info.id;
  const snap = view.snapshot;

  const balances = (snap?.balances ?? []).map((balance) =>
    el(
      "div",
      { className: "balance" },
      el("div", { className: "label" }, t("balance.account")),
      el("div", { className: "amount" }, formatMoney(balance.total)),
      balance.parts.length > 0 && el("div", { className: "muted small" }, partsText(balance)),
    ),
  );

  const windows = (snap?.windows ?? []).map((window) =>
    el(
      "div",
      { className: "window" },
      el(
        "div",
        { className: "window-head" },
        el("span", {}, windowTitle(window)),
        window.used &&
          window.limit &&
          el("span", { className: "muted" }, usedOf(window)),
      ),
      meter(window.usedPercent),
      el("div", { className: "muted small" }, windowStatus(window)),
    ),
  );

  const details =
    snap?.details.length > 0 &&
    el(
      "dl",
      { className: "details" },
      snap.details.map((detail) => [el("dt", {}, t(`detail.${detail.kind}`)), el("dd", {}, formatMoney(detail.value))]),
    );

  const notes = (snap?.notes ?? []).map((note) =>
    el("p", { className: `note small ${isWarning(note) ? "error" : "muted"}` }, noteText(note)),
  );

  const actions = el(
    "div",
    { className: "actions" },
    view.hasKey &&
      el("button", {
        textContent: t(view.refreshing ? "balance.refreshing" : "balance.refresh"),
        disabled: view.refreshing,
        onclick: () => invoke("refresh_balance", { providerId: id }),
      }),
    view.lastRefreshed && el("span", { className: "muted small" }, t("provider.updated", { time: localTime(view.lastRefreshed) })),
    el("span", { className: "spacer" }),
    el("button", {
      className: "link small",
      textContent: t("provider.openDashboard"),
      onclick: () => invoke("open_provider_page", { providerId: id, page: "dashboard" }),
    }),
  );

  const keyAction = editing.has(id)
    ? keyEditor(view)
    : el("button", {
        className: "link small",
        textContent: t(!view.hasKey ? "balance.addKey" : view.suggestsReplacingKey ? "balance.replaceKey" : "balance.changeKey"),
        onclick: () => edit(id, true),
      });

  return el(
    "div",
    { className: "provider-card" },
    snap?.plan && el("div", { className: "label" }, t("provider.plan", { plan: snap.plan })),
    balances,
    windows,
    details,
    notes,
    view.error && el("p", { className: "error small" }, view.error),
    showsPricing(view) && el("div", { id: "pricing-slot" }),
    view.peak && peakBlock(view),
    actions,
    keyAction,
  );
}

// Peak hours of providers with a plain weekly clock (Z.AI); DeepSeek's are the pricing section.

/** "UTC+8", "UTC−3:30". */
function zoneName(hours) {
  const sign = hours < 0 ? "−" : "+";
  const whole = Math.trunc(Math.abs(hours));
  const minutes = Math.round((Math.abs(hours) - whole) * 60);
  return `UTC${sign}${whole}${minutes ? `:${pad(minutes)}` : ""}`;
}

/** The time of `iso` on a clock `hours` ahead of UTC. */
const zonedTime = (iso, hours) =>
  new Date(Date.parse(iso) + hours * 3600000).toLocaleTimeString(i18n.locale, {
    timeZone: "UTC",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });

const minuteTime = (minutes) => `${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
const multiplier = (value) => new Intl.NumberFormat(i18n.locale, { maximumFractionDigits: 1 }).format(value);

function peakBlock(view) {
  const id = view.info.id;
  const { peak } = view;
  const { utcOffsetHours, ranges } = view.info.peakHours;
  const rates = view.snapshot?.peakRates;
  const zone = zoneName(utcOffsetHours);
  const next = t("panel.nextAt", {
    period: periodName(peak.nextPeriod),
    time: `${zonedTime(peak.nextTransition, utcOffsetHours)} ${zone}`,
  });
  const sameZone = new Date().getTimezoneOffset() === -utcOffsetHours * 60;
  const spans = new Intl.ListFormat(i18n.locale, { type: "conjunction" }).format(
    ranges.map((range) => `${minuteTime(range.start)}–${minuteTime(range.end)}`),
  );
  return el(
    "div",
    { className: `peak-hours ${peak.period}` },
    el("div", { className: "label" }, t("peak.title")),
    el(
      "div",
      { className: "hero" },
      el(
        "div",
        { className: "hero-text" },
        el("h2", {}, periodName(peak.period)),
        rates &&
          el("p", { className: "muted small" }, t("peak.rates", { peak: multiplier(rates.peak), offPeak: multiplier(rates.offPeak) })),
      ),
      rates && el("span", { className: "badge" }, `×${multiplier(peak.period === "peak" ? rates.peak : rates.offPeak)}`),
    ),
    el("div", { className: "countdown", id: `peak-countdown-${id}` }, "--:--:--"),
    el(
      "div",
      { className: "muted" },
      sameZone ? next : `${next} · ${t("panel.yourTime", { time: localTime(peak.nextTransition) })}`,
    ),
    el("div", { className: "progress" }, el("div", { id: `peak-progress-${id}` })),
    el("p", { className: "muted small" }, t("peak.schedule", { ranges: spans, zone })),
  );
}

/** Moves every peak countdown; reloads once one reaches its switch. */
function tickPeaks(now) {
  for (const view of providers) {
    if (!view.peak) continue;
    const next = Date.parse(view.peak.nextTransition);
    const start = Date.parse(view.peak.intervalStart);
    const seconds = Math.max(0, Math.floor((next - now) / 1000));
    const countdown = $(`peak-countdown-${view.info.id}`);
    if (countdown) countdown.textContent = clock(seconds);
    const bar = $(`peak-progress-${view.info.id}`);
    if (bar) bar.style.width = `${Math.min(100, Math.max(0, ((now - start) / (next - start)) * 100))}%`;
    if (seconds === 0) reload();
  }
}

function renderAddProviders(views, overviewEmpty) {
  const section = $("add-providers");
  section.hidden = views.length === 0;
  const children = [
    overviewEmpty && el("p", { className: "muted" }, t("overview.empty")),
    views.length > 0 && el("div", { className: "label" }, t("overview.add")),
    ...views.map((view) =>
      el(
        "div",
        { className: "add-item" },
        el(
          "div",
          { className: "add-head" },
          providerIcon(view.info),
          el("span", { className: "row-name" }, view.info.name),
          !editing.has(view.info.id) &&
            el("button", { className: "link small", textContent: t("balance.addKey"), onclick: () => edit(view.info.id, true) }),
        ),
        editing.has(view.info.id) && keyEditor(view),
      ),
    ),
  ];
  section.replaceChildren(...children.filter(Boolean));
}

function keyEditor(view) {
  const id = view.info.id;
  const input = el("input", {
    type: "password",
    placeholder: view.info.keyPlaceholder,
    autocomplete: "off",
    spellcheck: false,
  });
  const message = el("p", { className: "error small" });
  const save = async () => {
    try {
      await invoke("save_api_key", { providerId: id, key: input.value });
      editing.delete(id);
      expanded = id;
      saveExpanded();
    } catch (error) {
      message.textContent = String(error);
    }
  };
  input.addEventListener("keydown", (e) => e.key === "Enter" && save());
  queueMicrotask(() => input.focus());

  return el(
    "div",
    { className: "key-editor" },
    el("div", { className: "row" }, input, el("button", { className: "primary", textContent: t("balance.save"), onclick: save })),
    el(
      "div",
      { className: "row" },
      el("button", { className: "link small", textContent: t("balance.cancel"), onclick: () => edit(id, false) }),
      el("button", {
        className: "link small",
        textContent: t("provider.getKey"),
        onclick: () => invoke("open_provider_page", { providerId: id, page: "keys" }),
      }),
      view.hasKey &&
        el("button", {
          className: "link small error",
          textContent: t("balance.removeKey"),
          onclick: () => invoke("remove_api_key", { providerId: id }).then(() => editing.delete(id)),
        }),
    ),
    message,
  );
}

function edit(id, open) {
  open ? editing.add(id) : editing.delete(id);
  renderProviders(providers);
}

// Calendar

/** Today's date in Beijing as `YYYY-MM-DD`. */
const beijingToday = () => new Intl.DateTimeFormat("en-CA", { timeZone: BEIJING }).format(new Date());
/** Today's date where the user is, as `YYYY-MM-DD`. */
const localToday = () => new Intl.DateTimeFormat("en-CA").format(new Date());
/** The month shows the user's today; the week, in Beijing hours, shows Beijing's. */
const calendarToday = () => (calendarView === "month" ? localToday() : beijingToday());

/** Dates are handled as UTC midnights so the local time zone never shifts them. */
const parseDay = (day) => new Date(`${day}T00:00:00Z`);
const formatDay = (date) => date.toISOString().slice(0, 10);

function moveCursor(step) {
  const date = parseDay(calendarCursor ?? calendarToday());
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
  const cursor = calendarCursor ?? calendarToday();
  const date = parseDay(cursor);
  $("legend-off").textContent = t(calendarView === "month" ? "legend.offAllDay" : "legend.off");
  $("legend-holiday").hidden = calendarView !== "month";
  if (calendarView === "month") {
    const days = await invoke("month_calendar", { year: date.getUTCFullYear(), month: date.getUTCMonth() + 1 });
    $("cal-title").textContent = date.toLocaleDateString(i18n.locale, { month: "long", year: "numeric", timeZone: "UTC" });
    $("calendar").replaceChildren(renderMonth(days));
  } else {
    const days = await invoke("week_grid", { date: cursor });
    const short = (day) => parseDay(day).toLocaleDateString(i18n.locale, { month: "short", day: "numeric", timeZone: "UTC" });
    $("cal-title").textContent = `${short(days[0].date)} – ${short(days[6].date)}`;
    $("calendar").replaceChildren(renderWeek(days));
  }
}

const weekdayName = (day, style) =>
  parseDay(day).toLocaleDateString(i18n.locale, { weekday: style, timeZone: "UTC" });

function renderMonth(days) {
  const grid = el("div", { className: "month" });
  // 2024-01-01 was a Monday.
  for (let i = 0; i < 7; i++) {
    grid.append(el("div", { className: "head" }, weekdayName(`2024-01-0${i + 1}`, "narrow")));
  }
  const leading = (parseDay(days[0].date).getUTCDay() + 6) % 7;
  for (let i = 0; i < leading; i++) grid.append(el("div"));

  // The month marks the user's own date: west of Beijing, Beijing's date runs a day ahead every
  // evening. (The week grid stays in Beijing hours, so it marks Beijing's day and hour.)
  const today = localToday();
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
listen("providers", (e) => renderProviders(e.payload));
listen("settings", (e) => {
  settings = e.payload;
  renderSettings();
  renderProviders(providers);
});
listen("panel-shown", () => {
  // Opening the panel goes back to today.
  calendarCursor = null;
  reload();
  Aquarium.wake();
});
// A hidden window may keep running timers; the aquarium stops until the panel opens again.
listen("panel-hidden", () => Aquarium.sleep());
document.addEventListener("visibilitychange", () => (document.hidden ? Aquarium.sleep() : Aquarium.wake()));

// Preferences

function renderSettings() {
  $("setting-pricing").checked = settings.showsDeepseekPricing;
  $("setting-countdown").checked = settings.showsCountdown;
  $("setting-countdown").disabled = !settings.showsDeepseekPricing;
  $("setting-login").checked = settings.launchAtLogin;
  $("setting-aquarium").checked = settings.animatesAquarium;
  Aquarium.setAnimated(settings.animatesAquarium);
}

$("settings-toggle").addEventListener("click", () => {
  const open = $("settings").hidden;
  $("settings").hidden = !open;
  $("settings-toggle").setAttribute("aria-expanded", String(open));
});
$("setting-pricing").addEventListener("change", (e) =>
  invoke("update_settings", { showsDeepseekPricing: e.target.checked }),
);
$("setting-countdown").addEventListener("change", (e) => invoke("update_settings", { showsCountdown: e.target.checked }));
$("setting-aquarium").addEventListener("change", (e) =>
  invoke("update_settings", { animatesAquarium: e.target.checked }),
);
$("setting-login").addEventListener("change", (e) =>
  invoke("set_launch_at_login", { enabled: e.target.checked }).catch((error) => {
    console.error("couldn't change launch at login", error);
    renderSettings();
  }),
);

$("quit").addEventListener("click", () => invoke("quit"));
document.addEventListener("keydown", (e) => e.key === "Escape" && invoke("hide_panel"));

// The window takes the height of its content; the backend caps it at the screen height.
new ResizeObserver(([entry]) => {
  invoke("resize_panel", { height: entry.target.getBoundingClientRect().height });
}).observe(document.querySelector("main"));

async function init() {
  try {
    i18n = await invoke("get_i18n");
  } catch (error) {
    console.error("couldn't load UI strings", error);
  }
  applyStaticStrings();
  Aquarium.attach($("aquarium"));
  setInterval(tick, 1000);
  await reload();
}

init();
