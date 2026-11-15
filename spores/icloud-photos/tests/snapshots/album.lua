-- The second listing screenshot: one album open, in the album's own order.
--
-- Rendered from this package's own `ui/photos.lua` with
-- `lua tools/shot.lua ui/photos.lua tests/snapshots/album.lua …`, so the
-- screenshot is reproducible from the package bytes.
return {
  source = "iCloud",
  count = 84,
  album = "AL1",
  album_name = "Trip to Skye",
  albums = {
    { id = "AL1", name = "Trip to Skye", count = 84, folder = false },
    { id = "AL2", name = "Kitchen renovation", count = 212, folder = false },
  },
  months = {
    { key = 202608, label = "August 2026", assets = {
      { id = "P21", day = "14 Aug", aspect = 1.5, kind = "image", filename = "IMG_5120.HEIC",
        thumb = "blake3:1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809" },
      { id = "P22", day = "14 Aug", aspect = 1.333, kind = "image", favorite = true,
        filename = "IMG_5121.HEIC", thumb = nil },
      { id = "P23", day = "13 Aug", aspect = 0.75, kind = "live", filename = "IMG_5118.HEIC", thumb = nil },
      { id = "P24", day = "13 Aug", aspect = 1, kind = "image", filename = "IMG_5110.JPG", thumb = nil },
      { id = "P25", day = "11 Aug", aspect = 1.5, kind = "video", filename = "IMG_5090.MOV", thumb = nil },
    } },
  },
}
