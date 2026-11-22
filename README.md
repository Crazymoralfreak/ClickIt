# ClickIt

[Русский](README.ru.md)

An auto-clicker for Windows. A scenario is built from steps: click, move, drag, scroll, key, text input, pause, wait for a condition, "If / else" and a "Repeat" block. The same scenario can be built on the canvas: blocks are joined by links, so branches and loops look like a diagram. Scenarios can be recorded from the mouse and keyboard, combined into queues and run on a schedule.

The interface is available in German, English, Spanish, French, Japanese, Korean, Polish, Portuguese, Russian, Turkish and Simplified Chinese. The application has no account and no cloud. It uses the network for updates and, if you write to the assistant yourself, for the model address from the settings.

## Download

The latest version is on the [releases](https://github.com/Crazymoralfreak/ClickIt/releases/latest) page:

- `ClickIt_<version>_x64-setup.exe` — the installer. It installs the application for the current user, no administrator rights needed. It puts `clickit-mcp.exe` next to the program.
- `ClickIt_<version>_x64-portable.exe` — the portable version, runs without installation. On start it puts `clickit-mcp.exe` into the same folder.

The files are not signed, so Windows SmartScreen shows a warning on the first start.

## Updates

On start, the installed version asks GitHub whether a new version is out. A found update is downloaded, its signature is checked, and it installs by itself when no run or recording is in progress; after the installation ClickIt starts again. Scenarios and settings are kept.

- The check is turned off with the "Check for updates at startup" box in the settings. Without it ClickIt does not contact GitHub on its own. The assistant's requests to the model are set separately.
- You can check by hand with the "Check for updates" button in the "About" section.
- The portable version does not update itself: it only reports that a new version is out.
- Version 0.1.0 cannot update. A new installer has to be installed over it by hand, once.

## Connecting the assistant

A scenario can be built from outside or from the "Assistant" page. Both entrances call the same actions: list and edit scenarios, convert a list to a canvas, check a condition, take a screenshot, read a point's colour, template, journal, status, run, the trace of the last run and stop.

The external entrance is the `clickit-mcp` program. It is started by an external program that speaks MCP and brings its own model. ClickIt does not call that model. The program passes requests to the running ClickIt over a local pipe. The pipe is open only while "Allow external connections" is on in the settings, in the "Assistant" block; it is off by default. While ClickIt is not running or the box is unticked, the tool answers with an error instead of staying silent. The words of the conversation stay with the external program. The actions ClickIt performed are shown on the "Assistant" page, on the "Connections" tab.

`clickit-mcp.exe` does not need to be downloaded separately. The installer puts it into the program folder. The portable version writes it next to itself on start. ClickIt does not write the external program's settings file itself. The "Assistant" page already has the text for this copy: the path to `clickit-mcp.exe` next to the running ClickIt, which you can copy and paste in place of the empty `"mcpServers": {}`. The form is:

```json
"mcpServers": {
  "clickit": {
    "command": "path to clickit-mcp.exe"
  }
}
```

The assistant reads and saves a canvas scenario as a graph. The `get_scenario` tool returns `format`: `list` or `canvas`. A canvas has `graph` instead of `steps`:

```json
"graph": {
  "nodes": [
    { "id": "s", "type": "start" },
    {
      "id": "w",
      "type": "wait",
      "title": "Wait for the button",
      "condition": { "kind": "imageFound", "template": "tpl-button.png", "area": { "kind": "screen" }, "threshold": 0.9 },
      "pollMs": 200,
      "timeoutMs": 10000
    },
    {
      "id": "c",
      "type": "click",
      "title": "Click",
      "button": "left",
      "action": "click",
      "position": { "kind": "absolute", "x": 640, "y": 360 }
    }
  ],
  "edges": [
    { "from": "s", "port": "next", "to": "w" },
    { "from": "w", "port": "next", "to": "c" }
  ],
  "notes": []
}
```

The other block fields are optional: `enabled`, `pauseAfter`, `repeat` and the place on the canvas `x`, `y`. ClickIt places a block without a place by itself. The `save_scenario` tool lists the block types and their outputs in its description. One link leaves one output. An output without a link ends the pass; a `timeout` output without a link stops the scenario with a failure.

The canvas has four more actions:

- `convert_scenario` turns a list into a graph, in the scenario itself or in a copy;
- `check_condition` looks at a block's condition once and clicks nothing;
- `run_scenario` with `startNode` makes a trial pass from the chosen block;
- `get_trace` returns the trace of the last run: how many times each block and link was passed and on which block the run ended.

The assistant's edit is visible at once on an open canvas: the changed blocks are highlighted for a few seconds, and the edit itself is undone with Ctrl+Z.

The "Assistant" page itself holds a conversation with a model at an address in the OpenAI format. Conversations are split into chats: one tab per chat, "+" creates a new one, the cross deletes. The settings have presets for the Ollama address (`http://127.0.0.1:11434/v1`) and LM Studio (`http://127.0.0.1:1234/v1`), as well as your own address and key for a cloud model. An empty address means the page does not use the network. A request is sent only when a message is sent. The key is stored in `%AppData%\ClickIt\agent.key`, not in the settings and not in the journal. Screenshots and the scenario text go to this address.

The "Start scenarios without confirmation" box is off by default and is shared by both entrances. While it is off, a start waits for an answer in the ClickIt window. When it is on, the scenario starts without a question, after the countdown, and the application presses keys by itself. Stopping asks no question.

This box does not apply to a scenario with a "Command" step: its start always waits for an answer, and the question shows the text of the commands. Saving a scenario to which the assistant or an external program added a new command waits for an answer in the same way.

In this version an external connection cannot change queues and the schedule, record the mouse and keyboard, or delete scenarios.

## Features

- Two scenario modes: "Whole screen" (absolute coordinates, the default) and "Window" (coordinates inside the chosen window, a pause while another window is in the foreground).
- Conditions: pixel colour, image found or gone, window title, running process.
- Repeats on four levels: step, "Repeat" block, scenario, scenario queue.
- Canvas: a scenario as a diagram of blocks and links, with step-by-step runs and the trace of the last run. More in the "Canvas" section.
- Schedule: once, daily, on days of the week, at an interval.
- Tray icon, hotkeys for start and stop, point capture, pausing the recording and cutting an image while recording; stopping by moving the cursor to the top-left corner.
- The "Do not run while running" list: while a program from the list is running, a scenario does not start and a running one pauses.
- Export and import of a scenario as a single `.clickit` file.
- Scenario folders with nesting, dragging scenarios and folders with the mouse, search in the list.
- The "Command" step and block: starting a program with arguments or the text of a script (cmd, PowerShell), with waiting for the end and a timeout. Running commands is off until it is switched on in the settings; before importing a file with commands, ClickIt shows them and asks whether to add the scenario.

## Canvas

A scenario is stored in one of two kinds: a list of steps or a canvas. The kind is chosen at creation. A finished list opens on the canvas with the "Open on the canvas" button: "Convert" changes the scenario itself and keeps the list so it can be returned with the "Return the list" button, "Convert a copy" creates a new scenario next to it. A graph built on the canvas cannot be converted back to a list.

- Action blocks are the same as the steps of a list. They are joined by "Start", "Activate window", "Condition" with the "yes" and "no" outputs, "Counter" and "While" with the "body" and "done" outputs, "Run scenario" and "Intercept". A check can be a single one or an "and" / "or" of two.
- One link leaves one output, a block can have any number of inputs. A loop is a link back to a block already passed.
- "Intercept" watches for an obstacle on top of the main flow, for example a window that popped up. It is checked before every block and at every poll of a wait, but no more often than its own interval. After its branch the interrupted block starts again.
- Frames and notes divide a scenario into parts. A collapsed frame looks like a single block. "Move out to a scenario" turns the selected blocks into a separate scenario and puts a "Run scenario" block in their place.
- Snippets are ready-made pieces in the list of blocks. Your own are saved with the "Save as a snippet" button and are kept in `%AppData%\ClickIt\snippets`.
- During a run the working block is outlined, and a dot runs along the passed link. After a stop a trace remains: the number of passes of each block and the block on which the run ended.
- "Run from here" makes one pass from the chosen block. Breakpoints and the "Step by step" button pause the run before a block; the keys for a step and for continuing are set in the settings, F10 and F9 by default.
- "Check now" in a condition form looks at the screen once and shows whether the condition holds; for an image, where it was found and with what similarity.

A canvas scenario is exported to `.clickit` version 2. Versions of ClickIt without the canvas do not open such a file.

## Limitations

ClickIt sends ordinary Windows input (`SendInput`) and looks at the screen image. It does not hide its process and does not mask the input. Another program may take into account the very presence of an automation tool on the computer; there is no guarantee that it will not apply sanctions.

Windows started as administrator may not accept input from an ordinary process.

## Requirements

- Windows 10 version 1809 or newer, or Windows 11, x64.
- The WebView2 runtime. Windows 11 always has it. On a machine without WebView2 the installer downloads it by itself; the portable version will not start without it.

## Data

Scenarios, queues, the schedule, settings and the journal are kept in `%AppData%\ClickIt`. The installed and portable versions use the same folder, and only one instance runs at a time.

The "Data" block in the settings has separate cleanup buttons: the journal, chats with the model, connection records, unused images, backup lists, the window cache and leftovers. They do not touch scenarios, queues, the schedule or settings.

When the program is uninstalled the folder stays. A separate box in the uninstall window erases it; it is off by default.

## Build

You need:

- Node.js 20.19 or newer (or 22.12 and newer);
- Rust 1.85 or newer with the `x86_64-pc-windows-msvc` toolchain;
- Microsoft C++ Build Tools.

```
npm install
npm run tauri build
```

After the build:

- installer: `src-tauri\target\release\bundle\nsis\ClickIt_<version>_x64-setup.exe`;
- portable version: `src-tauri\target\release\clickit.exe`, the file can be copied to any folder.

The installer installs the application for the current user, without administrator rights. The language of its pages is chosen by the Windows language. A desktop shortcut is offered on the last page, and the box is off by default. A silent installation has no such page, and the shortcut is created; the `/NS` switch turns it off.

Development mode:

```
npm run tauri dev
```

## Releasing a version

1. Set the new version number in three files: `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`.
2. Commit the changes and put the tag `v<version>` on that commit, for example `v0.2.0`.
3. Push the tag to GitHub.

The tag starts `.github/workflows/release.yml`: it checks that the number in the tag matches the number in the three files, runs the unit tests, builds and signs the installer, builds the portable version and attaches three files to the release of that tag: the installer, the portable version and `latest.json`. If the release does not exist yet, it is created with an automatic list of changes.

`latest.json` is the description of the update: the version number, the link to the installer and its signature. Installed copies read it at `releases/latest/download/latest.json`, so the update is seen only for the release that GitHub considers the latest: not a draft and not a pre-release.

### Update signing key

The application installs only an update whose signature matches the public key in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key and its password are not stored in the repository. They are kept in the GitHub Actions secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

If the private key is lost, already installed copies cannot be updated: a version with a new public key has to be released and installed by hand.

Signing is switched on by the file `src-tauri/tauri.release.conf.json`, which only the release build uses. An ordinary local build does not need the key; its installer is not signed.

## Checks

```
npm run typecheck
```

In the `src-tauri` folder:

```
cargo clippy --tests
cargo test --lib
```

The unit tests cover logic without system calls: validation of scenarios, graphs and queues, graph traversal, converting a list to a graph, the schedule, normalisation of a recording, import and export, dictionaries. Input, capture during recording, the tray, hotkeys, the installer and the update are checked only in the running application.

## Structure

- `src-tauri/src` — the Rust core: model and validation (`model`), storage (`storage`), input (`input`), windows and processes (`target`), screen capture and template search (`screen`), the executor (`engine`), recording (`recorder`), the schedule (`scheduler`), tray, hotkeys and windows (`shell`), import and export (`transfer`), updates (`update`), the assistant (`harness`, `agent`), commands for the interface (`commands`). The console `clickit-mcp` is `src-tauri/src/bin/clickit-mcp.rs`.
- `src` — the interface in React and TypeScript: pages in `features`, dictionaries in `i18n`, types — a mirror of the Rust model — in `types`. The canvas is `src/features/canvas`, built on `@xyflow/react`; the graph model in the core is `src-tauri/src/model/graph.rs`.
- `src-tauri/windows/hooks.nsh` — additions to the NSIS installer.
