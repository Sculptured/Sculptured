# Docs screenshots

`capture.mjs` takes every screenshot in `../images/{quickstart,guides,import}`, and the README's board picture `../../docs/images/board.png`, from the real, running app. Nothing is drawn or mocked. Re-run it after a UI change. It uses the app's defaults: the Coastal look and the Cards board style, at 1280×800 (the README board at 1440×860, twice the pixels).

1. Build the app and its web UI: `cargo build -p sculptured && npm --prefix web run build`.
2. Make an empty data folder and a home folder with an `app` folder in it (the demo session "ran" there), for example `/tmp/docsd` and `/tmp/docshome/app`. Keep the data folder's path short: the agents' tool socket lives in it, and socket paths are limited to about 100 characters.
3. Make the fixture CLI, outside the repository. "Improve this team with AI" (Settings › More settings) asks a CLI, so for the screenshots a stand-in answers with one canned team, and no model is called:

   ```sh
   #!/bin/sh
   [ "$1" = --version ] && echo "fixture 1.0" && exit 0
   cat > /dev/null
   cat /path/to/teams.json   # {"type":"result","structured_output":{"alternatives":[{"label":"team","rationale":"…","team":{…}}]}}
   ```

4. Serve it on a spare port, from the repository root (it serves `web/dist`):
   `HOME=<home> target/debug/sculptured serve --port 4499 --data-dir <data> --claude-program <fixture CLI> --codex-program /bin/false`
5. Capture: `npm run screenshots -- --base http://127.0.0.1:4499 --session-folder <home>/app`. Add `--only <step>` to retake one group (`startFrom`, `websiteLaunch`, `manyChildren`, `importSession`, `readmeBoard`), or `--from <step>` to go on from one. Each run creates its teams again, so start from an empty data folder for a clean Projects list. Stop the server when done.
6. Look at every image before committing: the state it shows, nothing clipped, and nothing personal (only `~/app` paths, made-up teams).

`--base` is required, so the script never drives a Sculptured you actually use. The teams are Practice agents, so no model is called.
