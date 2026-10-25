# neat-adblock

A small C ABI (`neat_adblock.dll`) around [adblock-rust](https://github.com/brave/adblock-rust)
for NEAT Browser (WinUI 3 + WebView2). It loads a
[neat-filters](https://github.com/neatbrowser/neat-filters) release and answers:

- should this network request be blocked, redirected, or rewritten?
- should this popup (new window) be blocked?
- extra CSP directives for a document
- cosmetic resources (hide selectors, procedural actions, scriptlets) for a page

`include/neat_adblock.h` is the contract. `cs/` has a P/Invoke wrapper.

## Status

| Part | State |
| --- | --- |
| Rust library | builds; unit test; exercised through the C ABI by `tests/smoke.py` against a real release (22 checks, including 8 threads on one engine) on Linux |
| Windows build (x64, arm64) | workflow written in `.github/workflows/build.yml`, **not run yet** |
| `cs/NeatAdblock.cs`, `cs/WebView2Mapping.cs` | **not compiled or run** (no .NET SDK was available) |
| WebView2 wiring | described below, not implemented |

## API summary

```c
NeatEngine* neat_engine_create(const char* dir, char** error_out);   // release dir -> engine
void        neat_engine_free(NeatEngine*);

int32_t neat_check_request(engine, url, source_url, request_type, method, NeatCheckResult*);
void    neat_check_result_free(NeatCheckResult*);                    // action: allow / block / redirect
int32_t neat_check_popup(engine, target_url, opener_url);            // 1 = block the new window
int32_t neat_get_csp(engine, url, source_url, request_type, char** csp_out);
int32_t neat_cosmetic_resources(engine, url, char** json_out);
int32_t neat_hidden_class_id_selectors(engine, classes_json, ids_json, exceptions_json, char** json_out);
void    neat_string_free(char*);
```

Details and ownership rules are in the header. Every function is safe to call concurrently on
one engine, and a Rust panic never crosses the boundary (it becomes `NEAT_ERR_INTERNAL`).

`neat_engine_create` reads `manifest.json` from the directory, puts `role: "popup"` lists into a
second engine, puts all other lists into the main engine, and applies the "trusted" permission to
lists marked `trusted: true` (so only those can use trusted uBO scriptlets). **Verify the files'
sha256 against the manifest before calling it**; the DLL does not.

## Build

```sh
cargo build --release                          # target/release/libneat_adblock.so (Linux)
cargo build --release --target x86_64-pc-windows-msvc    # neat_adblock.dll (x64)
cargo build --release --target aarch64-pc-windows-msvc   # arm64
```

The MSVC build links the C runtime statically (`.cargo/config.toml`), so the DLL does not need the
VC++ redistributable. `adblock` is pinned to `=0.13.3` to match neat-filters' validator; bump both
together.

`single-thread` (an adblock-rust default feature) is off on purpose: it would make the engine
`!Send`/`!Sync`, and the point of this library is to build a new engine on a background thread and
swap it in while WebView2 events keep calling the old one.

## Test

```sh
cargo build --release
python3 -I tests/smoke.py target/release/libneat_adblock.so ../neat-filters/dist
```

(`dist/` comes from `node scripts/build.mjs` in neat-filters, or from a downloaded release.)

## Wiring it into NEAT

1. **Updating.** On start and every few hours: fetch `manifest.json`, download changed files to a
   new temp directory (copying unchanged files from the current one), verify sha256, then
   `Task.Run(() => AdblockEngine.Create(dir))`. When it returns, swap the field
   (`Interlocked.Exchange`) and dispose the old engine. In-flight calls keep it alive.
   Ship one release inside the installer so the first run and offline runs still block.
2. **Network.** `CoreWebView2.AddWebResourceRequestedFilter("*", All)`; in `WebResourceRequested`
   call `Check(uri, currentPageUrl, WebView2Mapping.RequestType(ctx, isMain))`. Block = respond
   with a 403/empty `CreateWebResourceResponse`; redirect = respond with the data: URL body;
   `RewrittenUrl` = re-issue to the new URL. Whether a Document request is the main frame or an
   iframe is not in the event args, so main-frame loads are best checked in `NavigationStarting`.
3. **Popups.** In `NewWindowRequested`, if `ShouldBlockPopup(e.Uri, sender.Source)` set
   `e.Handled = true`. Optionally also block when `!e.IsUserInitiated`.
4. **Cosmetic.** On navigation, call `CosmeticResourcesJson(url)`, inject `hide_selectors` as
   CSS (`display: none !important`), run `injected_script` with
   `AddScriptToExecuteOnDocumentCreatedAsync`, and run `procedural_actions` with your own runtime.
   Frames need their own call with their own URL.

Not covered by adblock-rust or this library: `$replace`, HTML filters (`##^`), `$urlskip`,
`$ipaddress`, `$cname`. See neat-filters' README for the measured list.
