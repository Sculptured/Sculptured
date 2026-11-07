# Docs screenshots

`capture.mjs` takes every screenshot in `../images/{quickstart,guides,import}`, and the README's board picture `../../docs/images/board.png`, from the real, running app. Nothing is drawn or mocked. Re-run it after a UI change. It uses the app's defaults: the Coastal look and the Cards board style, at 1280×800 (the README board at 1440×860, twice the pixels).

1. Build the app and its web UI: `cargo build -p sculptured && npm --prefix web run build`.
2. Make an empty data folder and a home folder with an `app` folder in it (the demo session "ran" there), for example `/tmp/docsd` and `/tmp/docshome/app`. Keep the data folder's path short: the agents' tool socket lives in it, and socket paths are limited to about 100 characters.
3. Sign Claude Code in for that home folder (`HOME=<home> claude`, then `/login`), or export `ANTHROPIC_API_KEY` in the shell that serves. Capturing runs real Claude Code agents on haiku, and "Improve this team with AI" asks the real Claude Code, so it **costs credits**. The script stops each team once its pictures are taken.
4. Serve the app on a spare port, from the repository root (it serves `web/dist`):
   `HOME=<home> target/debug/sculptured serve --port 4499 --data-dir <data> --codex-program /bin/false`
5. Capture: `npm run screenshots -- --base http://127.0.0.1:4499 --session-folder <home>/app`. Add `--only <step>` to retake one group (`startFrom`, `websiteLaunch`, `manyChildren`, `importSession`, `readmeBoard`), or `--from <step>` to go on from one. Each run creates its teams again (and spends credits again), so start from an empty data folder for a clean Projects list. Stop the server when done.
6. Look at every image before committing: the state it shows, nothing clipped, and nothing personal (only `~/app` paths, made-up teams).

`--base` is required, so the script never drives a Sculptured you actually use. Every agent is a real Claude Code agent on haiku working in `<home>/app`, so the agents' replies and the time each step takes vary from run to run; the script waits for what it needs (up to 4 minutes per step) instead of fixed pauses.
