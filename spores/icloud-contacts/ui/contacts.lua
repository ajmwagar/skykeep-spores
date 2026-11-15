-- Skykeep Contacts, drawn on Canvas.
--
-- Ported from Skykeep's contacts window (src/screens/contacts): a rail of names
-- on the left, the whole card on the right. The view owns composition only — the
-- module answered the document and the host owns the controls it can press.
--
-- Nothing here names a path or a network: a contact is a record, and the rail is
-- that collection sorted the way Contacts sorts it.
local ui = assert(_G.ui or require("fungos.ui"), "ui library not composed")

local M = { name = "org.skykeep.contacts", version = "0.1.0" }

-- Sizes in logical pixels. The host's theme wins when it has one; the fallback
-- is the same one every Spore in this family starts from.
local DEFAULT_THEME = { title = 28, large = 22, body = 16, small = 13, micro = 10, radius = 10 }

local function theme()
  return (_G.module and _G.module.theme) or DEFAULT_THEME
end

-- A document with no contacts still draws a window, not a blank one.
local FIXTURE = {
  source = "iCloud",
  count = 3,
  selected = "c-1",
  contacts = {
    { id = "c-1", name = "Ada Lovelace", initials = "AL", email = "ada@acme.example",
      phone = "+1 555 0100", organization = "Acme" },
    { id = "c-2", name = "Grace Hopper", initials = "GH", email = "grace@navy.example" },
    { id = "c-3", name = "Émile Zola", initials = "EZ", organization = "" },
  },
  contact = {
    id = "c-1", name = "Ada Lovelace", initials = "AL",
    given_name = "Ada", family_name = "Lovelace", organization = "Acme",
    title = "Mathematician", birthday = "December 10",
    emails = { { label = "work", value = "ada@acme.example" } },
    phones = { { label = "mobile", value = "+1 555 0100" } },
    addresses = {}, urls = {},
    note = "Wrote the first program.", uid = "c-1",
  },
}

local function slug(value)
  value = tostring(value or "")
  return (value:gsub("[^%w%-_]", "-")):sub(1, 48)
end

local function chosen(view)
  for _, contact in ipairs(view.contacts or {}) do
    if contact.id == view.selected then return contact end
  end
  return (view.contacts or {})[1]
end

local function monogram(contact)
  return ui.card({
    id = "mark-" .. slug(contact.id),
    w = 56,
    h = 56,
    pad = "sm",
    children = {
      ui.text(contact.initials or "?", { id = "mark-text-" .. slug(contact.id), style = "heading", ink = "primary" }),
    },
  })
end

-- The window's height in pixels, so the panes can fill it. `grow` only absorbs
-- space a parent has already been given a height for, so the two panes are told
-- how tall they are instead: header, gaps and padding taken off the window.
local function surface_height()
  local _, height = ui.viewport_size()
  return math.max(560, math.floor(height / 4) * 4)
end

local function body_height()
  return math.max(360, surface_height() - 112)
end

-- The rail: a label, then a row per contact, quiet until it is the one shown.
local function rail_column(view)
  local children = {
    ui.text("CONTACTS", { id = "rail-label", style = "caption", ink = "muted" }),
    ui.rule({ id = "rail-rule" }),
  }
  for _, contact in ipairs(view.contacts or {}) do
    children[#children + 1] = ui.button(contact.name or contact.id, {
      id = "rail-" .. slug(contact.id),
      small = true,
      quiet = contact.id ~= view.selected,
      action = ui.action("contacts.select", { id = contact.id }),
    })
  end
  if #children == 2 then
    children[#children + 1] = ui.text("No contacts yet.", { id = "rail-empty", style = "caption", ink = "muted" })
  end
  return ui.column({ id = "rail", h = body_height(), gap = "xs", children = children })
end

-- One labelled band of methods: the card's "email", "phone" sections.
local function methods(title, values, id)
  if #values == 0 then return nil end
  local children = {
    ui.text(title, { id = id .. "-label", style = "caption", ink = "muted" }),
  }
  for index, value in ipairs(values) do
    local label = value.label or ""
    children[#children + 1] = ui.text(
      label ~= "" and (label .. "  " .. tostring(value.value)) or tostring(value.value),
      { id = id .. "-" .. index, style = "body", ink = "text" })
  end
  return ui.column({ id = id, gap = "xs", children = children })
end

local function card(view)
  local contact = view.contact or chosen(view) or {}
  local children = {
    ui.row({ id = "card-head", widths = { 0.15, 0.85 }, gap = "md", children = {
      monogram(contact),
      ui.column({ id = "card-name", gap = "xs", children = {
        ui.text(contact.name or "Contact", { id = "card-title", style = "title", ink = "text" }),
        ui.text(contact.organization or "", { id = "card-org", style = "caption", ink = "muted" }),
        ui.text(contact.title or "", { id = "card-role", style = "caption", ink = "muted" }),
      } }),
    } }),
    ui.rule({ id = "card-rule" }),
  }
  local sections = {
    methods("EMAIL", contact.emails or {}, "card-email"),
    methods("PHONE", contact.phones or {}, "card-phone"),
    methods("ADDRESS", contact.addresses or {}, "card-address"),
    methods("WEBSITE", contact.urls or {}, "card-url"),
  }
  for _, section in ipairs(sections) do
    if section then children[#children + 1] = section end
  end
  if (contact.birthday or "") ~= "" then
    children[#children + 1] = ui.column({ id = "card-birthday", gap = "xs", children = {
      ui.text("BIRTHDAY", { style = "caption", ink = "muted" }),
      ui.text(tostring(contact.birthday), { id = "card-birthday-value", style = "body", ink = "text" }),
    } })
  end
  if (contact.note or "") ~= "" then
    children[#children + 1] = ui.column({ id = "card-note", gap = "xs", children = {
      ui.text("NOTE", { style = "caption", ink = "muted" }),
      ui.text(tostring(contact.note), { id = "card-note-value", style = "body", ink = "text" }),
    } })
  end
  if (contact.uid or "") ~= "" then
    children[#children + 1] = ui.text("record " .. tostring(contact.uid), {
      id = "card-uid", style = "caption", ink = "muted",
    })
  end
  return ui.card({ id = "card", h = body_height(), pad = "md", gap = "sm", children = children })
end

local function header(view)
  local count = view.count or #(view.contacts or {})
  return ui.row({ id = "head", widths = { 0.7, 0.3 }, children = {
    ui.column({ id = "head-title", gap = "xs", children = {
      ui.text("Contacts", { id = "title", style = "title", ink = "text" }),
      ui.text(string.format("%s · %d people", view.source or "iCloud", count), {
        id = "source", style = "caption", ink = "muted",
      }),
    } }),
    ui.column({ id = "head-actions", gap = "xs", children = {
      ui.button("Refresh", { id = "refresh", small = true, quiet = true }),
    } }),
  } })
end

function M.render(view)
  if not view or not view.contacts then view = FIXTURE end
  M.hits = ui.present(ui.column({ id = "contacts-root", h = surface_height(), pad = "md", gap = "md", children = {
    header(view),
    ui.row({ id = "body", widths = { 0.26, 0.74 }, gap = "md", children = {
      rail_column(view),
      card(view),
    } }),
  } }), theme())
end

return M
