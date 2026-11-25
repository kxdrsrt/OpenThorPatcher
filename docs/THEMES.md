# Themes, news and changelog

## News and changelog (the operator writes them)

Both are part of `patchlist.json`, so they are signed and need no hosting of their own; they arrive with the next
publish. **Patch Maker → News & changelog** edits them; in `otp-project.json`:

```json
"news": [{ "date": "2026-11-25", "title": "Welcome", "body": "The **test server** is open.\n\n- 75x base\n- [Discord](https://discord.gg/x)" }],
"changelog": [{ "date": "2026-11-25", "version": "Patch 12", "title": "LGP colours", "items": ["Magnus tiles are red"] }]
```

News text: blank line = paragraph, `- ` = list item, `**bold**`, `[text](https://…)`. Everything else is escaped. A
`website` (https) adds a **Website** button that opens the server's site in the player's browser, `links` ([{title,url}]: Discord, forum, vote …) appear at the bottom of the window, and links inside news text open in the browser too (only http/https, never inside the patcher). A `news_url` (https) adds a third tab that shows a website inside the patcher; without written news the website opens first.
Newest first.

## Themes

The patcher's look is a folder, **`patcher-theme/` inside the client folder**:

```
patcher-theme/
  theme.json     { "name": "My theme", "author": "me", "css": "theme.css", "logo": "logo.png", "vars": { "--accent": "#e9c46a" } }
  theme.css      any CSS; url(file) of files in this folder is inlined, so a theme is just a folder
  logo.png       replaces the logo at the top
  fonts/…        optional fonts (declare them with @font-face in theme.css)
```

No folder = the built-in **Midgard Night** (night-blue Ragnarok look, steel frame, gold lettering, Cinzel headings, the
OpenThor logo that sticks out over the top edge of the window). Because the folder lives in the client, you ship and update
it with the patcher itself: put it into the master client and add its files on the Patch Maker's *Files* page.

Start from the built-in theme: **Patch Maker → News & changelog → Export the default theme…** writes `patcher-theme/`
with `theme.css`, `theme.json`, the logo and the font.

### What a theme can style

Classes: `.logo` `.frame` `.bar` `.who` `.wincontrols` `.wbtn` `.body` `.left` `.tabs` `.box` `.pane` `.news-item`
`.change-entry` `.right` `.state` `.progress` `.pline` `.out` `.buttons` `.btn` `.btn.primary` `.foot` `dialog`.
Variables: colours (`--bg --surface --text --muted --accent --accent-ink --ok --warn --danger --line`), `--display` (title
font), `--logo-w` / `--logo-h` (logo size), `--overhang` (how far the logo sticks out above the frame; the window is
transparent there). The layout itself (`patcher.css`) only places things.

### Limits

* Fonts and images must be files in the theme folder (the window does not load anything from the internet except the
  optional news website).
* A theme is CSS, not code: nothing in a theme can run scripts.
* The window is fixed (988×672), frameless and transparent around the frame (no native border or shadow). Every shadow of
  the frame has to fit into the transparent margin (`--margin`, 30 px), otherwise the window rectangle becomes visible.
* The logo shipped with the program was drawn with an image generator in the style of the Ragnarok logo; replace it freely.
  The Cinzel font is under the SIL Open Font License (`fonts/OFL.txt`).
