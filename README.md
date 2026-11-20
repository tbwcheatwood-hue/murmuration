# Murmuration

Murmuration is a desktop studio for a mesh of LM Studio machines. TensorMesh is the orchestrator. SwarmLink is the path between them: LM Studio LM Link, with Tailscale used only when it is installed and only as extra address information.

The node list is the signed-in LM Link. `lms link status` names this device and the other devices on that account. Machines that are not on the link are left off the list. Nothing in that filter deletes model files on disk.

![Murmuration topology with the LM Link devices for this account](docs/images/topology.png)

The screenshot above is the desktop app on a real account. FamilyRoom is this computer. GamingPC and Timmys-PC are the other devices LM Link reported. The gateway bearer token is not in the picture.

## What you can do from the window

The studio is four zones.

| Zone | What it shows |
| --- | --- |
| Left | LM Link devices, the model registry, swarm blueprints, and extensions |
| Center | Topology, the workbench, and the traffic inspector |
| Right | Decode settings and local MCP tools |
| Bottom | Combined VRAM and logs from lms, the gateway, and discovery |

![LM Link list and the workbench](docs/images/workbench.png)

Each LM Link row shows whether the device is online, whether it is this computer, and whether it is the preferred device. VRAM appears only when a local GPU probe returns it. Refresh asks `lms link status` again.

The workbench takes one objective, splits it across roles, and keeps the resulting artifact editable. The traffic tab records the routing decision, time to first token, and whether the reply came from a live LM Studio or from the orchestrator brief.

The command palette is Ctrl+K (Cmd+K on macOS). Ctrl+B toggles the sidebar, Ctrl+J toggles the log dock, and Ctrl+1, Ctrl+2, and Ctrl+3 switch the center tabs.

## Run it

Requirements: Node 20, Rust, and the Tauri prerequisites. LM Studio supplies the device list. A GPU probe uses `nvidia-smi` when that command exists.

```bash
npm install
npm run tauri dev
```

`npm run dev` opens the browser preview only. That preview cannot run `lms`, so the node list stays empty there and says so. The desktop shell is what reads LM Link.

A release build without an installer:

```bash
npm run tauri build -- --no-bundle
```

The executable is `src-tauri/target/release/tensormesh.exe`. The window title is Murmuration.

## Point a client at the mesh

The gateway binds to loopback only, on port 11435, and walks forward to 11444 if that port is taken. The title bar shows the port that opened.

```text
http://127.0.0.1:11435/v1
```

Use the model id `murmuration`. Cursor, Continue, and other OpenAI-compatible clients can treat that URL as one local model.

| Route | Behavior |
| --- | --- |
| `GET /v1/models` | The virtual Murmuration model plus the catalog |
| `POST /v1/chat/completions` | Routed to a live LM Studio when one is up, otherwise an orchestrator brief |
| `POST /v1/embeddings` | A small local lexical vector, labeled as such |
| `GET /metrics` | Prometheus text for the swarm |
| `POST /mcp` | Local tools. Requires the bearer token shown in the inspector |

Chat completions that reach a live LM Studio are proxied. If that hop fails, the gateway logs the failure and answers with the orchestrator brief instead of pretending the peer ran.

## How a device gets onto the list

Membership comes from one command:

```bash
lms link status --json
```

The desktop app parses that JSON. This device stays first. Every peer in the payload is added, including a peer that was missing from the previous list. A device that is not in the payload is removed from the window. Simulated demo machines are not members and are not shown.

The command does not include Tailscale addresses or GPU memory. Those stay blank until a separate probe fills them in. On this computer, `nvidia-smi` and `tailscale` were not on `PATH`, so the cards say VRAM is unknown and the log says Tailscale status is not available. That is a missing probe, not a guessed device.

If `lms link status` cannot be read, the list does not invent peers. It shows the error and keeps any devices already confirmed on the link.

## Routing, VRAM, and sharding

Three modes change how a prompt is placed:

- **Pipeline** sends the architect, coder, and critic steps to different devices when those roles are healthy.
- **Shard** computes a layer placement and reserves VRAM for it. Weights are not split across machines until you set an `http` or `https` shard backend, such as a llama.cpp RPC server. The log says when the map is only a plan.
- **Speculative** prefers a smaller device as the draft and a larger one as the target.

Before a mount, the fit calculator estimates weight, KV cache, and scratch against the pool. KV size grows with context length and with the cache quant (fp16, q8, or q4). If the pool cannot hold the model, commit is rejected.

A node that is offline, sleeping, or thermally throttled is skipped. The next prompt fails over, and the inspector shows which device was skipped.

## MCP tools

`POST /mcp` on the gateway requires `Authorization: Bearer <token>`. The token is generated when the app starts and is shown only in the inspector.

| Tool | What it does |
| --- | --- |
| `swarm_status` | Current nodes, mode, and gateway |
| `files_read`, `files_write` | Paths must stay under the project root you set |
| `shell_exec` | Stays off until a project root is set and the shell is armed |
| `vector_query` | Lexical overlap over the catalog, recent logs, and recent traffic |
| `web_search` | Uses `TENSORMESH_SEARCH_URL` when that environment variable is an http(s) endpoint. Otherwise it returns local hits and says search is not configured |

Exit-node changes and git worktree commits both require an explicit confirm. The worktree helper refuses paths that contain `..`.

## Extensions

Three built-ins ship as SwarmPlugin manifests:

- **Prometheus exporter** copies the metrics text and shows the scrape URL.
- **Smart context compressor** summarizes the current objective onto the smallest warm device.
- **Git worktree auto-committer** writes an artifact into a worktree only after you confirm.

A tiny WebAssembly probe in the extensions panel reports its module name. It does not load remote code.

## Layout

```text
src/                  React studio
  components/         Activity bar, LM Link list, topology, workbench, inspector
  lib/                Browser-side mesh, fit math, gateway client
src-tauri/src/        Discovery, telemetry, fit, orchestrator, gateway, MCP
extensions/           SwarmPlugin manifests
docs/images/          README screenshots
```

Rust discovery tests cover two rules: devices outside the LM Link are dropped from the list, and devices present in `lms link status` are added when the list did not already have them. Unrelated model records stay.

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## What this build does not do

- It does not download weights or call `lms load` unless you mount a real local file and the command is allowed to run.
- It does not split tensors across the network unless the shard backend URL is set.
- It does not change Tailscale exit nodes unless you confirm that action.
- The browser preview is not a second source of devices. It waits for the desktop shell.
