# Service icons

Glyphs for the services in a fleet file's `icon:` field, by name (`icon: jellyfin` is
`jellyfin.svg`). They come from [Simple Icons](https://simpleicons.org) 16.38.1, CC0: single-path
glyphs on one 24x24 grid, so they line up. Each logo is a trademark of its project.

Menagerie draws every service the same way: the glyph in the text colour on a rounded tile. A
service without a glyph here (Seerr, Prowlarr and Bazarr aren't in Simple Icons) gets a monogram,
the first letter of its name, on the same tile.

To add one, copy `icons/<slug>.svg` from the `simple-icons` package, save it under the name the
fleet file uses, and check it has no scripts or external references.
