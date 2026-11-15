-- Skykeep Photos, drawn on Canvas.
--
-- Ported from Skykeep's photos window (src/screens/photos): albums down the
-- left, the library by month on the right. The composition is the part worth
-- sharing; the pixels are not here yet. A tile draws a mark, the day and the
-- content id, because resolving that id to bytes is the host's job and the
-- missing piece is `vfs/cid` (see spores/VFS-CID.md).
local ui = assert(_G.ui or require("fungos.ui"), "ui library not composed")

local M = { name = "org.skykeep.photos", version = "0.1.0" }

local DEFAULT_THEME = { title = 28, large = 22, body = 16, small = 13, micro = 10, radius = 10 }

local function theme()
  return (_G.module and _G.module.theme) or DEFAULT_THEME
end

local FIXTURE = {
  source = "iCloud",
  count = 2403,
  album = nil,
  albums = {
    { id = "lib", name = "Library", count = 2403, folder = false },
    { id = "AL1", name = "Trip to Skye", count = 84, folder = false },
    { id = "AL2", name = "Kitchen renovation", count = 212, folder = false },
    { id = "F1", name = "Folders", count = 0, folder = true },
  },
  months = {
    { key = 202603, label = "March 2026", assets = {
      { id = "P1", day = "20 Mar", aspect = 1.333, favorite = true, kind = "image",
        filename = "IMG_4821.HEIC", thumb = "blake3:6e1c8f0d2b7a4c5e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4" },
      { id = "P2", day = "20 Mar", aspect = 0.75, kind = "video",
        filename = "IMG_4822.MOV", thumb = "blake3:7f2d9e1c3a5b7d9f0e2c4a6b8d0f1e3c5a7b9d1f3e5c7a9b0d2f4e6c8a0b2d4" },
      { id = "P3", day = "18 Mar", aspect = 1, kind = "live",
        filename = "IMG_4815.HEIC", thumb = nil },
      { id = "P4", day = "12 Mar", aspect = 1.5, kind = "image",
        filename = "IMG_4790.JPG", thumb = nil },
    } },
  },
}

local function slug(value)
  return (tostring(value or ""):gsub("[^%w%-_]", "-"):sub(1, 48))
end

local function surface_height()
  local _, height = ui.viewport_size()
  return math.max(560, math.floor(height / 4) * 4)
end

-- A picture is its aspect ratio, until the host can hand one over: a portrait
-- tile is taller than a landscape one, so the grid reads as a photo library
-- before a single byte is served.
local function tile_height(aspect)
  local height = 150 / (tonumber(aspect) or 1)
  return math.max(96, math.min(232, math.floor(height / 4) * 4))
end

local function mark(asset)
  if asset.kind == "video" then return "VID" end
  if asset.kind == "live" then return "LIV" end
  return "IMG"
end

local function tile(asset, height)
  local caption = asset.day or ""
  if asset.favorite then caption = caption .. "  *" end
  local children = {
    ui.button(mark(asset), {
      id = "tile-" .. slug(asset.id),
      quiet = true,
      action = ui.action("photos.open", { id = asset.id }),
    }),
    ui.text(caption, { id = "tile-day-" .. slug(asset.id), style = "caption", ink = "muted" }),
  }
  if asset.filename then
    children[#children + 1] = ui.text(tostring(asset.filename), {
      id = "tile-name-" .. slug(asset.id), style = "caption", ink = "text",
    })
  end
  return ui.card({ id = "tile-card-" .. slug(asset.id), h = height, pad = "xs",
    gap = "xs", children = children })
end

-- Six across, left-aligned: a short last row keeps its tiles the same width
-- instead of stretching three pictures across the window. Every tile in a row
-- shares the row's height, taken from the widest picture in it, because a row
-- is what the layout engine can size — a ragged bottom edge is the price of
-- per-picture aspect ratios, and the price of not having them is a grid that
-- stops looking like a library.
local function rows_of(assets)
  local rows = {}
  for index = 1, #assets, 6 do
    local row = {}
    local height = 96
    for offset = 0, 5 do
      local asset = assets[index + offset]
      if not asset then break end
      height = math.max(height, tile_height(asset.aspect))
      row[#row + 1] = asset
    end
    local tiles = {}
    for position, asset in ipairs(row) do
      tiles[position] = tile(asset, height)
    end
    local widths = {}
    for position = 1, #tiles do widths[position] = 1 / 6 end
    rows[#rows + 1] = ui.row({ id = "row-" .. index, widths = widths, gap = "xs", children = tiles })
  end
  return rows
end

local function month_block(month)
  local children = {
    ui.text(month.label or "", { id = "month-label-" .. slug(month.key), style = "heading", ink = "text" }),
  }
  for _, row in ipairs(rows_of(month.assets or {})) do
    children[#children + 1] = row
  end
  return ui.column({ id = "month-" .. slug(month.key), gap = "sm", children = children })
end

local function sidebar(view)
  local children = {
    ui.text("ALBUMS", { id = "albums-label", style = "caption", ink = "muted" }),
    ui.rule({ id = "albums-rule" }),
    ui.button("Library", {
      id = "album-library",
      small = true,
      quiet = view.album ~= nil,
      action = ui.action("photos.library", {}),
    }),
  }
  for _, album in ipairs(view.albums or {}) do
    if album.id ~= "lib" then
      children[#children + 1] = ui.button(string.format("%s  ·  %d", album.name or album.id, album.count or 0), {
        id = "album-" .. slug(album.id),
        small = true,
        quiet = album.id ~= view.album,
        action = ui.action("photos.album", { id = album.id }),
      })
    end
  end
  return ui.column({ id = "sidebar", h = surface_height() - 112, gap = "xs", children = children })
end

function M.render(view)
  if not view or not view.months then view = FIXTURE end
  local title = view.album_name or "Photos"
  local months = {}
  for _, month in ipairs(view.months or {}) do
    months[#months + 1] = month_block(month)
  end
  M.hits = ui.present(ui.column({ id = "photos-root", h = surface_height(), pad = "md", gap = "md", children = {
    ui.row({ id = "head", widths = { 0.7, 0.3 }, children = {
      ui.column({ id = "head-title", gap = "xs", children = {
        ui.text(title, { id = "title", style = "title", ink = "text" }),
        ui.text(string.format("%s · %d photos", view.source or "iCloud", view.count or 0), {
          id = "source", style = "caption", ink = "muted",
        }),
      } }),
    } }),
    ui.rule({ id = "head-rule" }),
    ui.row({ id = "body", widths = { 0.2, 0.8 }, gap = "md", children = {
      sidebar(view),
      ui.column({ id = "grid", gap = "lg", children = months }),
    } }),
  } }), theme())
end

return M
