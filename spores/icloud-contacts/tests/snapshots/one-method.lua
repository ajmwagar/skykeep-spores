-- The second listing screenshot: what a card looks like when most of it is empty.
-- A contact with one method is still a contact, and the view has to say so
-- rather than draw a blank card.
--
-- Rendered from this package's own `ui/contacts.lua` with
-- `lua tools/shot.lua ui/contacts.lua tests/snapshots/one-method.lua …`, so the
-- screenshot is reproducible from the package bytes.
return {
  source = "iCloud",
  count = 3,
  selected = "c-3",
  contacts = {
    { id = "c-1", name = "Ada Lovelace", initials = "AL", email = "ada@acme.example" },
    { id = "c-2", name = "Grace Hopper", initials = "GH", email = "grace@navy.example" },
    { id = "c-3", name = "Émile Zola", initials = "EZ" },
  },
  contact = {
    id = "c-3",
    name = "Émile Zola",
    initials = "EZ",
    given_name = "Émile",
    family_name = "Zola",
    organization = "",
    title = "",
    emails = {},
    phones = { { label = "mobile", value = "+33 1 23 45 67 89" } },
    addresses = {},
    urls = {},
    note = "",
    birthday = "April 2",
    uid = "c-3",
  },
}
