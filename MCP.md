# Use Zapture from an AI agent

Zapture includes a **local stdio MCP server**. Launch the same executable with `--mcp`. It uses JSON-RPC 2.0 and negotiates legacy MCP protocol `2025-06-18`. An agent/client must support this protocol and local stdio servers; "any AI agent" is not a claim that every product permits custom tools.

## Client configuration

Adapt this standard example to your client's MCP settings and executable location:

```json
{
  "mcpServers": {
    "zapture": {
      "command": "C:\\Apps\\Zapture\\Zapture.exe",
      "args": ["--mcp"]
    }
  }
}
```

Linux command example: `/home/you/Apps/zapture`.
macOS command example: `/Applications/Zapture.app/Contents/MacOS/Zapture`.
Keep `args` as `["--mcp"]`. A Linux/macOS preview needs the FFmpeg prerequisites in README. On macOS, set device indices in Zapture's Settings before agent-driven recording in that session. Launch in an active graphical login session, not a headless SSH session.

On Windows close another ordinary Zapture instance before letting the MCP client launch its instance. The single-instance guard prevents two recorders from competing. Do not launch both ordinary and MCP instances at the same time. The floating controls remain visible and usable by the person at the computer. Stdio is the only transport: no listening TCP port, API key or server account.

## Tools

| Tool | Arguments | Result |
|---|---|---|
| `zapture_status` | `{}` | State, output folder, last/current recording path, warnings/platform limits |
| `zapture_screenshot` | `{"inline": true}` | Saves PNG, returns local path plus MCP PNG image content (up to 12 MiB). Does not change clipboard. |
| `zapture_start_recording` | `{"audio": "off"}` | Starts recording and returns state; audio can be `off`, `system`, `mic`, `both` |
| `zapture_stop_recording` | `{}` | Requests stop; poll status until `idle` before using the completed file |

Windows agent screenshots capture the primary monitor. Windows recording uses the current adjustable capture frame. Unix recording uses the desktop/configured screen. Unix previews may include the floating controls in captures. The macOS/Linux adapter can report `recording` immediately after FFmpeg starts, then report a failure at the next status check; inspect `message` and the `.log` file. Real-device capture validation is pending.

## Example agent instruction

> Use Zapture to take a screenshot and inspect it. If recording is requested, start with audio off unless I explicitly request sound. Stop when done, poll until idle, then report the saved path. Do not claim a recording succeeded just because a process started.

## Direct protocol example

Write each JSON object as one newline-delimited line to the child process stdin, keeping the process and stdin open between calls:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"my-agent","version":"1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"zapture_screenshot","arguments":{"inline":true}}}
```

Stdout carries only protocol messages; diagnostics go to stderr. Incoming lines are limited to 1 MiB; unknown tools and unexpected arguments are rejected. Tool failures use `isError: true`. A timed-out action may already have started: inspect status before retrying. Stop and finalize before closing stdin. On orderly stdin EOF the server requests graceful application exit; abrupt client/process termination cannot guarantee MP4 finalization.

## Scope

Screen capture and recording only. No mouse clicks, typing, arbitrary shell execution, arbitrary file reading, credential access, network uploads, remote listening or permission bypass. All screen/microphone permissions remain enforced by the operating system. Launching the MCP server grants its local client access to these capture tools; only configure it in clients you intend to use for this purpose.

## Validation

Unit-tested: initialization, tool listing/calls, tool-error formatting, input allowlists, audio-mode validation and notifications. Windows cross-compilation and Linux compilation are checked separately. End-to-end capture through a real MCP client, Windows/macOS permissions and hardware audio behavior still require device testing.
