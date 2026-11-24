# Visual QA

`npm run qa` (from `web/`) runs `screenshots.mjs`: it opens the UI in a headless Chromium and
writes one screenshot per view and theme here, as `<shot>-<light|dark>.png`.

```sh
npm run qa -- [baseUrl] [authBaseUrl]   # baseUrl default http://127.0.0.1:4317
SHOTS=overview,files npm run qa         # only these shots
```

- Mocked shots (a live run built from `../src/fixtures/review-failed.json` and
  `fixtures/live-transcript.json`) work against any running zerochat.
- Real shots need run ids from your machine in `QA_*` variables; a shot without them is skipped.
  The header of `screenshots.mjs` lists them.
- Needs a Chromium: set `CHROMIUM`, or have Playwright's cache in `~/.cache/ms-playwright`.

The PNGs are output, not sources: `web/.gitignore` ignores `qa/*.png`, so they are not committed.
Look at them, then delete them.
