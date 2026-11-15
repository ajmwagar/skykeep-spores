-- Skykeep Calendar, drawn on Canvas.
--
-- Ported from Skykeep's calendar window (src/screens/calendar): one week, seven
-- days, every calendar's events side by side. Skykeep draws a month grid too;
-- this is the week, because that is the view whose layout is worth sharing
-- before the data model is settled.
--
-- The document names the neighbouring weeks, so the chevrons carry a date and the
-- module does the arithmetic. Nothing here counts days.
local ui = assert(_G.ui or require("fungos.ui"), "ui library not composed")

local M = { name = "org.skykeep.calendar", version = "0.1.0" }

local DEFAULT_THEME = { title = 28, large = 22, body = 16, small = 13, micro = 10, radius = 10 }

local function theme()
  return (_G.module and _G.module.theme) or DEFAULT_THEME
end

local FIXTURE = {
  source = "iCloud",
  count = 4,
  week = { start = "2026-03-16", ["end"] = "2026-03-22", label = "16 - 22 March 2026",
    prev = "2026-03-09", next = "2026-03-23" },
  calendars = { { name = "Home", count = 2 }, { name = "Work", count = 2 } },
  days = {
    { date = "2026-03-16", label = "MON", day = 16, month = "March",
      all_day = { { id = "e0", title = "Bin day", calendar = "Home" } },
      events = { { id = "e1", title = "Standup", time = "09:30", location = "Studio", calendar = "Work" } } },
    { date = "2026-03-17", label = "TUE", day = 17, month = "March", all_day = {}, events = {} },
    { date = "2026-03-18", label = "WED", day = 18, month = "March", all_day = {},
      events = { { id = "e2", title = "Dentist", time = "11:00", location = "Bridge Street", calendar = "Home" } } },
    { date = "2026-03-19", label = "THU", day = 19, month = "March", all_day = {}, events = {} },
    { date = "2026-03-20", label = "FRI", day = 20, month = "March", all_day = {},
      events = { { id = "e3", title = "Release review", time = "15:00", calendar = "Work" } } },
    { date = "2026-03-21", label = "SAT", day = 21, month = "March", all_day = {}, events = {} },
    { date = "2026-03-22", label = "SUN", day = 22, month = "March", all_day = {}, events = {} },
  },
}

local function slug(value)
  return (tostring(value or ""):gsub("[^%w%-_]", "-"):sub(1, 48))
end

local function surface_height()
  local _, height = ui.viewport_size()
  return math.max(560, math.floor(height / 4) * 4)
end

local function event_row(item, day)
  local label = item.title or "(no title)"
  if item.time and item.time ~= "" then
    label = item.time .. "  " .. label
  end
  return ui.button(label, {
    id = "event-" .. slug(item.id),
    small = true,
    quiet = true,
    action = ui.action("calendar.select", { id = item.id, date = day }),
  })
end

local function day_column(day)
  local children = {
    ui.text(day.label or "", { id = "day-name-" .. slug(day.date), style = "caption", ink = "muted" }),
    ui.text(tostring(day.day or ""), { id = "day-number-" .. slug(day.date), style = "heading", ink = "text" }),
  }
  for _, item in ipairs(day.all_day or {}) do
    children[#children + 1] = ui.text(item.title or "", {
      id = "allday-" .. slug(item.id), style = "caption", ink = "primary",
    })
  end
  for _, item in ipairs(day.events or {}) do
    children[#children + 1] = event_row(item, day.date)
  end
  return ui.column({ id = "day-" .. slug(day.date), gap = "xs", children = children })
end

local function header(view)
  local week = view.week or {}
  return ui.row({ id = "head", widths = { 0.62, 0.38 }, children = {
    ui.column({ id = "head-title", gap = "xs", children = {
      ui.text("Calendar", { id = "title", style = "title", ink = "text" }),
      ui.text(string.format("%s · %s · %d events", week.label or "",
        view.source or "iCloud", view.count or 0), {
        id = "source", style = "caption", ink = "muted",
      }),
    } }),
    ui.row({ id = "head-nav", widths = { 0.2, 0.2, 0.6 }, children = {
      ui.button("<", { id = "prev-week", small = true, quiet = true,
        action = ui.action("calendar.week", { date = week.prev }) }),
      ui.button(">", { id = "next-week", small = true, quiet = true,
        action = ui.action("calendar.week", { date = week.next }) }),
      ui.text("", { id = "head-spacer", style = "caption", ink = "muted" }),
    } }),
  } })
end

function M.render(view)
  if not view or not view.days then view = FIXTURE end
  local columns = {}
  for _, day in ipairs(view.days or {}) do
    columns[#columns + 1] = day_column(day)
  end
  M.hits = ui.present(ui.column({ id = "calendar-root", h = surface_height(), pad = "md", gap = "md", children = {
    header(view),
    ui.rule({ id = "head-rule" }),
    ui.row({ id = "week", widths = { 1 / 7, 1 / 7, 1 / 7, 1 / 7, 1 / 7, 1 / 7, 1 / 7 }, gap = "sm", children = columns }),
  } }), theme())
end

return M
