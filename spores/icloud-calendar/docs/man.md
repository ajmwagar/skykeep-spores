# icloud-calendar

Your iCloud calendars, one week at a time.

```text
icloud-calendar                   the first week the library holds
icloud-calendar view <YYYY-MM-DD> the week containing that day
icloud-calendar list              calendars, and how many events each holds
icloud-calendar event <uid>       one event, unabridged
```

## What it reads

The ``fungos.calendar/event`` collection: one record per event, iCalendar-shaped
(`summary`, `description`, `location`, `start`, `end`, `all_day`, `timezone`,
`rrule`, `url`). That is the shape `fungos-records` already defines and the
feeds importer already writes, so an event from a subscribed feed and an event
typed into iCloud draw the same.

`start` and `end` are RFC 3339 strings in UTC. Two things follow, and both are
why this module has no date dependency:

- a week is a prefix test — `2026-03-16` through `2026-03-22` are string
  comparisons, not instants;
- events sort by `start` as strings, because every one has the same width.

Weekday arithmetic (which Monday a date belongs to) is Howard Hinnant's
`days_from_civil` / `civil_from_days`, twenty lines instead of a calendar crate.

## The week it draws

Monday to Sunday. A caller that names no day gets the week of the earliest event
in the library: this Spore has no clock, and a view that read the system time
would draw a different week on every render.

The document names the week before and after, so the chevrons carry a date back
and the arithmetic stays on this side of the boundary.

## Limits

`scan` pages at 256 records and stops at 50,000 events. Recurrence is not
expanded: `rrule` travels in the record, and turning it into instances is the
sync half's job, not a view's.

## Licence

GPL-3.0-only. This Spore is a port of Skykeep's calendar window and travels with
that licence; see `NOTICE` at the root of this repository.
