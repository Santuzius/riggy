# Interfaces

## Command line

```text
riggy                                   show the avatar (the tray does this for you)
riggy install FILE ...                  install speech models (.gguf) and avatars (.glb)
riggy say [--language LANGUAGE] TEXT    speak TEXT and return when it has been played
```

`riggy install` puts `.gguf` files into the speech server's model folder and restarts the server (the tray starts its own server with them by itself), and `.glb` files into Riggy's avatars folder (see [avatars.md](avatars.md)). `riggy say` speaks like `riggy-mcp`, in the voice and tempo chosen in the tray.

## MCP server

`riggy-mcp` is a stdio MCP server with one tool, `speak(text, language)`, that reads a short text aloud and returns at once. It fetches the speech from the Qwen3-TTS server (`TTS_URL`, default `http://127.0.0.1:8902`), changes its tempo with ffmpeg and sends it to Riggy's speech socket, so the agent's replies come out of Riggy's mouth; while Riggy is closed, `pw-play` plays it. Texts are queued, each plays to its end, and at least 3 s of silence separate them. With "Start automatically › When an AI agent starts riggy-mcp" on, it starts the tray if none runs. `RIGGY_SOCKET` points it at another speech socket.

Its `initialize` reply carries `instructions` (from [instructions.md](../src/bin/riggy-mcp/instructions.md)): when the agent calls `speak` and what it passes. Clients that support this field, such as Claude Code, add them to the agent's prompt; `riggy-mcp --help` prints the same text for the others. "Advanced settings › Default MCP instructions" in the tray switches them off.

## Speech socket

Riggy listens on the Unix socket `$XDG_RUNTIME_DIR/riggy.sock` (`[speech] socket`), plays what it receives on the default sound output and moves the mouth with it. One connection carries one utterance:

1. one line of JSON: `{"text": "Hello", "language": "English", "rate": 24000}` (`rate` in Hz; all fields optional; the text method of lip sync needs `text` and `language`),
2. raw PCM, signed 16-bit little-endian mono, until the sender closes the connection.

Later connections wait until the earlier ones have been read.

## Control socket

The tray reaches the avatar through `$XDG_RUNTIME_DIR/riggy-control.sock`, next to the speech socket: one JSON command per line.

| Command | Effect |
|---|---|
| `{"click_through":true}` | clicks go through (`false`: dragging moves the window) |
| `{"lipsync":"text"}` | lip sync method `sound`, `rhubarb` or `text` from the next utterance on |
| `{"zoom":0.5}` | framing from the whole figure (0) to the face (1) |
| `{"turn":-20.0}` | turn of the figure in degrees, positive to its left |
| `{"play":"pack/clip"}` | plays an installed clip once |
| `{"hold":{"clip":"pack/clip","at":1.5}}` | shows an installed clip frozen at 1.5 s until another clip is played or held |
| `"quit"` | closes the avatar |
