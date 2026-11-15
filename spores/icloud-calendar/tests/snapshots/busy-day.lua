-- The second listing screenshot: a busy day beside five empty ones.
--
-- Rendered from this package's own `ui/calendar.lua` with
-- `lua tools/shot.lua ui/calendar.lua tests/snapshots/busy-day.lua …`, so the
-- screenshot is reproducible from the package bytes.
return {
  source = "iCloud",
  count = 6,
  week = { start = "2026-04-06", ["end"] = "2026-04-12", label = "6 - 12 April 2026",
    prev = "2026-03-30", next = "2026-04-13" },
  calendars = { { name = "Home", count = 3 }, { name = "Work", count = 3 } },
  days = {
    { date = "2026-04-06", label = "MON", day = 6, month = "April", all_day = {}, events = {} },
    { date = "2026-04-07", label = "TUE", day = 7, month = "April",
      all_day = { { id = "e0", title = "Bin day", calendar = "Home" } },
      events = {
        { id = "e1", title = "Standup", time = "09:30", calendar = "Work" },
        { id = "e2", title = "Dentist", time = "11:00", location = "Bridge Street", calendar = "Home" },
        { id = "e3", title = "Release review", time = "15:00", calendar = "Work" },
      } },
    { date = "2026-04-08", label = "WED", day = 8, month = "April", all_day = {}, events = {} },
    { date = "2026-04-09", label = "THU", day = 9, month = "April", all_day = {}, events = {} },
    { date = "2026-04-10", label = "FRI", day = 10, month = "April", all_day = {}, events = {} },
    { date = "2026-04-11", label = "SAT", day = 11, month = "April", all_day = {}, events = {} },
    { date = "2026-04-12", label = "SUN", day = 12, month = "April", all_day = {}, events = {} },
  },
}
