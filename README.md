# mcp-computer-use

An [MCP](https://modelcontextprotocol.io) server that drives the local desktop —
mouse, keyboard, and screen capture — for use as a computer-use tool by LLM
agents.

The shape of the tool is a deliberate, near-1:1 implementation of OpenAI's
[Computer Use (CUA) tool](https://platform.openai.com/docs/guides/tools-computer-use):
the action set, JSON schema, modifier-key conventions, and image payload
format are all chosen to match the GA spec so a CUA-aware model can drive this
server with no shimming. The server speaks JSON-RPC over stdio.

## What it does

- Exposes the OpenAI computer-use action set as MCP tools:
  `click`, `double_click`, `scroll`, `type`, `wait`, `keypress`, `drag`,
  `move`, and `screenshot`.
- Returns screenshots as OpenAI-compatible `input_image` content parts (either
  a `data:image/png;base64,...` URL or a path to a PNG on disk).
- Automatically downscales returned screenshots and **remaps** subsequent
  click/scroll/move/drag coordinates from image space back into absolute
  desktop space, so the model can target what it sees in the (smaller) image
  and still hit the right pixels on the real display.
- Runs cross-platform: input simulation via `enigo` (with native Wayland
  support on Linux); screen capture uses `windows-capture` on Windows,
  `libwayshot` on Linux/Wayland.

## Tool surface

The server can present its capabilities in one of two shapes, selected at
startup. Both shapes share a single backend, so behavior is identical — only
the JSON tool surface differs.

### Default mode — one tool, OpenAI-style

Exposes a single tool, `computer_use`, that takes an ordered `actions` array.
This is the most native fit for OpenAI's computer-use schema: an agent can
hand the same `actions[]` array it would send to the GA tool straight through
the MCP boundary.

```json
{
  "actions": [
    { "type": "screenshot" },
    { "type": "click", "button": "left", "x": 360, "y": 200 },
    { "type": "type", "text": "hello" },
    { "type": "keypress", "keys": ["enter"] }
  ]
}
```

### `--split` mode — one MCP tool per action

Exposes one MCP tool per action type: `computer_click`, `computer_double_click`,
`computer_scroll`, `computer_type`, `computer_wait`, `computer_keypress`,
`computer_drag`, `computer_move`, `computer_screenshot`. The action name is
implied by the tool name, so each request omits the `type` field. Useful for
clients that prefer fine-grained tool listings or strict per-tool permissioning,
and for older models that do better with one tool per action.

### Response shape

Every action returns an entry in `results[]`:

```json
{
  "results": [
    {
      "action": "screenshot",
      "status": "ok",
      "image": {
        "type": "input_image",
        "image_url": "data:image/png;base64,...",
        "width": 720,
        "height": 405,
        "physical_width": 1920,
        "physical_height": 1080,
        "scale_factor": 0.375
      }
    }
  ]
}
```

`status` is `"ok"` or `"error"` (with a `message`). `image` is only set by
`screenshot`. `width`/`height` are the pixel dimensions of the PNG actually
returned (the coordinate space the model should target);
`physical_width`/`physical_height` describe the underlying desktop;
`scale_factor` is `image / desktop` along the longest desktop axis. When
`--images-as-files` is set, `image_url` is replaced with a `path` to a PNG on
disk.

## Building and running

```bash
cargo build --release
./target/release/mcp-computer-use [flags]
```

The server speaks MCP over stdio, so it is normally launched by an MCP client
(Cursor, Claude Desktop, etc.) rather than run directly.

## Command-line flags

All flags are optional and order-independent.

| Flag | Default | Description |
| --- | --- | --- |
| `--split` | off (batched mode) | Expose one MCP tool per action (`computer_click`, `computer_type`, ...`) instead of the default `computer_use` tool with ordered `actions[]`. Useful for older models or clients that want per-tool permissioning. |
| `--max-image-dimension=<n>` | `900` | Cap the longest pixel dimension of returned screenshots to `<n>`. The server downscales captures past this size and transparently remaps later click/scroll/move/drag coordinates from image space back to absolute desktop space, so models can target what they see. Set `--max-image-dimension=0` to disable downscaling and return native resolution. |
| `--images-as-files` | off | Write screenshot PNGs under the OS temp dir (`{temp}/mcp-computer-use/`) and return a `path` field instead of inlining a `data:image/png;base64,...` URL. Useful when responses would otherwise be huge or when a client prefers loading images from disk. |

## Coordinate handling

When downscaling is active (the default), the server caches the most-recent
screenshot's image-to-desktop coordinate map. Any coordinate-bearing action
that follows (`click`, `double_click`, `scroll`, `move`, `drag`) has its `x`,
`y` values interpreted in the returned image's coordinate space and remapped
to absolute desktop coordinates before being sent to the input controller.
Before any screenshot has been taken — or with `--max-image-dimension=0` —
coordinates pass through unchanged.

## Modifier keys

`click`, `double_click`, `scroll`, `move`, and `drag` each accept an optional
`keys: ["ctrl", "shift", ...]` array of modifiers held for the duration of
the action. `keypress` takes a `keys` array describing the chord to press
(e.g. `["ctrl", "c"]`). `type` takes free-form `text`.

## Debug behavior

Debug builds (`cargo build` without `--release`) sleep ~3s between actions in
a batch, to make it easier to watch an agent operate the desktop in real
time. Release builds run actions back-to-back.

Each dispatched action is also logged as a single JSON line to **stderr**
(stdout is reserved for JSON-RPC), prefixed with `mcp-computer-use:`.
