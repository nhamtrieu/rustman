# Rustman

**English** | [Tiếng Việt](README.md)

A Postman-style API client written in Rust (egui + reqwest). Native, a single binary, no account or cloud required.

## Running

```bash
cargo run --release
```

On Linux you need the system libraries for winit/wgpu (X11/Wayland, xkbcommon). The file picker uses xdg-desktop-portal.

## Features

- **Requests**: GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS, query params synced both ways with the URL, headers, multiple tabs (right-click a tab: New Request, Duplicate Tab, Close / Force Close Tab, Close Other Tabs, Close All Tabs, Force Close All Tabs).
- **Body**: raw (JSON with highlighting, validation and Beautify; Text/XML/HTML/JS), x-www-form-urlencoded, form-data (text + file), binary, GraphQL.
- **Auth**: Bearer, Basic, API Key (header/query), OAuth 2.0, and "Inherit auth from parent" to use the collection's auth.
- **OAuth 2.0** (per request or collection): Authorization Code, Authorization Code (PKCE, SHA-256/plain), Implicit, Password Credentials and Client Credentials grants. *Get New Access Token* opens the browser; if the Callback URL is `http://localhost:<port>` or `127.0.0.1`, Rustman catches the redirect itself, otherwise paste the URL the browser lands on into the dialog. Client credentials can be sent as a Basic header or in the body; supports Scope, State, Audience, Resource and Refresh Token URL. The token goes into a header (custom prefix) or the `access_token` query param; shows expiry, *Refresh*/*Clear* buttons, and *Auto-refresh*: expired tokens are refreshed before sending (in the Collection Runner too). Token requests show up in the Console. `{{variables}}` work in every field.
- **Per-request settings**: timeout, follow redirects, verify SSL.
- **Proxy** (Settings → Proxy): use the system proxy (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`) or a custom one (host:port, apply to HTTP and/or HTTPS, Basic auth, bypass list: domains, `*.domain`, IPs, CIDRs; localhost is bypassed by default). Applies to every request: tabs, runner, `pm.sendRequest`, OAuth 2.0, MCP.
- **Certificates** (Settings → Certificates): add CA certificates (PEM files, trusted in addition to the built-in roots) and per-host client certificates for mutual TLS (`api.example.com`, `*.example.com`, optionally with `:port`), CRT + KEY in PEM format.
- **Scripts** (the *Scripts* tab of requests and collections): JavaScript run *Pre-request* (before sending) and *Post-response* (after the response arrives), with a Postman-compatible `pm` API. Collection scripts run before request scripts. See [Scripts](#scripts).
- **Test Results**: `pm.test` results (PASS/FAIL/SKIPPED, filterable by status) and script errors with line numbers.
- **Response**: status/time/size, pretty highlighted JSON (line-virtualized, so large responses stay smooth), Raw, Headers, Cookies, Info, Copy, Save to file.
- **Collections**: nested folders, drag and drop to reorder/move, rename/duplicate/delete, collection-level variables and auth.
- **Environments & variables**: `{{var}}` everywhere, precedence environment > collection > globals; dynamic variables `{{$guid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`, `{{$randomEmail}}`...; defined/undefined variables are highlighted green/red, hover to see the resolved value.
- **Console** (status bar → Console, or Ctrl+Alt+C): logs every request sent, with the resolved URL, request/response headers and bodies, timing, size and errors; filter by text or errors only; right-click to copy the URL/response.
- **Toolsets (MCP)**: expose saved requests as *tools* for AI agents (Claude Code, Cursor, VS Code…). Rustman runs a local MCP server at `http://localhost:<port>/mcp` (Streamable HTTP). The request `{{variables}}` the agent should fill in become tool parameters (with description, required or optional); the rest are resolved from the active environment / collection. The *Connect* button provides a ready-made `claude mcp add --transport http …` command and JSON config. Agent tool calls are logged.
- **Terminal** (status bar → Terminal, or Ctrl+\`): a real shell in a PTY (bash/zsh/PowerShell) sharing the panel with the Console. Supports 16/256/truecolor, arrow keys, Ctrl+C/D/Z…, bracketed paste, mouse-wheel scrollback, full-screen programs (vim, htop). Ctrl+Shift+C or the copy button copies the screen.
- **Cookie jar** shared across requests (Tools → Clear cookies to reset).
- **History** of sent requests.
- **Import**: Postman Collection v2.1 (including scripts; folder scripts are merged into the requests inside), Postman Environment, OpenAPI 3 / Swagger 2 (JSON or YAML, creates both a spec and a collection), cURL commands (you can paste one straight into the URL bar).
- **Specs**: edit OpenAPI (YAML/JSON) with an endpoint list and live syntax errors alongside; *Generate Collection* builds a collection (folders by tag, a `{{baseUrl}}` variable, path params as variables, query/headers, sample bodies from schemas).
- **Mocks**: local mock server (`http://localhost:<port>`). Each route has a method (or ANY), a path like `/users/:id` or `/files/*`, status, headers, body and delay. Bodies/headers can use `{{id}}` (path params), query params and dynamic variables such as `{{$guid}}`. CORS is open, incoming requests are logged, and route changes take effect while the server is running.
- **Datasets**: editable data tables, import CSV/JSON, export CSV. Column names are variable names.
- **Collection Runner** (right-click a collection → *Run collection*): pick requests, iterations, delay and a dataset (iteration n uses row n; columns override other variables). Requests run in order with their scripts: variables set by one request are visible to the next, and `pm.execution.setNextRequest` changes the order. A request *Passed* when all its tests pass (with no tests, when status < 400); per-test results are shown under each request. All requests and `console.log` output also go to the Console.
- **Documents**: Markdown notes (headings, lists, checklists, code blocks, **bold**, *italic*, `code`) with Edit/Preview modes.
- **Export**: Postman Collection v2.1 (including scripts), Postman Environment.
- **Code snippets** (right panel, syntax highlighted): 34 languages/libraries — C libcurl, C# HttpClient/RestSharp, cURL, Dart dio/http, Go, HTTP, Java OkHttp/Unirest, JavaScript Fetch/jQuery/XHR, Kotlin OkHttp, NodeJs Axios/Native/Request/Unirest, Objective-C, OCaml, PHP cURL/Guzzle/HTTP_Request2/pecl_http, PowerShell, Python http.client/Requests, R httr/RCurl, Ruby, Rust reqwest, HTTPie, wget, Swift. Options: indentation, follow redirects, long-form cURL options, line continuation character.
- Light/dark theme, workspace autosave.

## Command line (CI)

`rustman run` runs a collection without opening the app, like `newman run`:

```bash
rustman run collection.json -e dev.postman_environment.json -d users.csv \
  -r cli,junit --reporter-junit-export results.xml
```

- `<collection>`: a Postman Collection v2.1 file, or the name of a collection in the Rustman workspace. When taken from the workspace, the app's globals, proxy and certificates are used as well.
- `-e` environment file or workspace environment name; `-g` globals file; `-d` CSV/JSON file (or dataset name), one row per iteration; `-n` number of iterations.
- `--folder <name>` (repeatable) runs only that folder/request; `--env-var k=v`, `--global-var k=v`; `--delay-request`, `--timeout-request`, `-k/--insecure`, `--bail`.
- Certificates: `--ssl-extra-ca-certs`, `--ssl-client-cert`, `--ssl-client-key`. Proxy comes from `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`.
- Reports: `-r cli,json,junit`, `--reporter-json-export`, `--reporter-junit-export`; `--export-environment`, `--export-globals` write variables back after scripts run.
- Exit codes: `0` when everything passes, `1` on any test/script/request failure (`-x` to always return 0), `2` on invalid usage. Run `rustman --help` for the full list.

## Scripts

Scripts are JavaScript (embedded QuickJS engine). Each script runs in its own sandbox, limited to 30 seconds and 128 MB.

| API | Description |
|---|---|
| `pm.environment`, `pm.collectionVariables`, `pm.globals` | `get`, `set`, `unset`, `has`, `clear`, `toObject`, `replaceIn`. Changes are saved to the workspace. |
| `pm.variables` | Reads with precedence local > data > environment > collection > globals; `set` only lasts for the current request run. `replaceIn("{{x}}")`. |
| `pm.iterationData` | The dataset row of the current iteration (read-only). |
| `pm.request` | `method`, `url` (`toString`, `getHost`, `getPath`, `query.add/upsert/remove`), `headers` (`get`, `add`, `upsert`, `remove`), `body.raw`. Editable in Pre-request. |
| `pm.response` | `code`, `status`, `responseTime`, `responseSize`, `headers.get`, `cookies.get`, `text()`, `json()`. |
| `pm.test(name, fn)`, `pm.test.skip` | Declare tests. |
| `pm.expect(x)` | Chai-style assertions: `equal`, `eql`, `deep`, `not`, `a/an`, `include`, `property`, `nested.property`, `lengthOf`, `above/below/least/most/within`, `keys`, `members`, `oneOf`, `match`, `ok/true/false/null/undefined/empty/exist`, `throw`, `jsonSchema`. |
| `pm.response.to...` | `have.status(200)`, `have.header(name[, value])`, `have.body(...)`, `have.jsonBody(path, value)`, `have.jsonSchema(schema)`, `be.ok/success/clientError/serverError/error/notFound/json`. |
| `pm.sendRequest(req[, callback])` | Send an extra request (a URL or `{url, method, header, body}`); use `await` or a `(err, res)` callback. These requests also appear in the Console. |
| `pm.execution.setNextRequest(name \| null)` | Runner: jump to the request with that name, `null` to end the iteration. |
| `pm.execution.skipRequest()` | Pre-request: don't send this request. |
| `pm.info` | `requestName`, `requestId`, `iteration`, `iterationCount`, `eventName`. |
| `console.log/info/warn/error` | Write to the Console. |
| `require(name)` | The same libraries as the Postman sandbox, loaded on first use: `crypto-js` 4.2, `lodash` 4.17, `moment` 2.30, `tv4`, `ajv` 6, `chai` 4 (unmodified originals, licenses in `src/script/lib/LICENSES.md`); `uuid`, `buffer`, `querystring`, `util`, `xml2js` (`parseString`, `Parser`, common options), `atob`, `btoa`. |
| Globals | `CryptoJS`, `_` (lodash), `tv4`, `xml2Json(xml)`, `Buffer`, `TextEncoder`/`TextDecoder`, `crypto.getRandomValues`, `atob`/`btoa`. |
| Legacy syntax | `tests["name"] = condition`, `postman.setEnvironmentVariable`, `responseBody`, `responseCode`... |

Proxy/certificates not yet supported: SOCKS proxies, PFX/PKCS#12 files and password-protected private keys (convert to PEM with `openssl pkcs12 -in c.pfx -out c.pem -nodes`).

Not yet supported: `cheerio`, `csv-parse`, `postman-collection`, other Node modules (`url`, `crypto`, ...), `pm.vault`, folder-level scripts (merged into requests on import). Toolsets (MCP) send requests without running scripts.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl+Enter | Send request |
| Ctrl+S | Save request |
| Ctrl+T | New tab |
| Ctrl+W | Close tab |
| Ctrl+Alt+W | Close tab, discarding unsaved changes |
| Ctrl+O | Import |
| Shift+click / Ctrl+click (sidebar) | Select multiple requests / folders |
| Delete (sidebar) | Delete selected items (macOS: Cmd+Backspace) |
| Ctrl+\\ | Toggle sidebar |
| Ctrl+Alt+C | Toggle Console |
| Ctrl+\` | Toggle Terminal |

## Data

The workspace is saved to `~/.local/share/rustman/workspace.json` (Linux). Set `RUSTMAN_DATA_DIR=/some/path` to use another directory (portable mode).

## Code layout

| File | Contents |
|---|---|
| `src/main.rs` | Entry point: dispatches to `rustman run` (CLI) or opens the app window |
| `src/model.rs` | Data model (Request, Collection, Environment, ...) |
| `src/vars.rs` | `{{var}}` substitution |
| `src/http.rs` | Building and sending requests (reqwest) |
| `src/oauth2.rs` | OAuth 2.0: grants, PKCE, localhost callback listener, token refresh |
| `src/run.rs` | Collection run loop (shared by the Runner tab and `rustman run`) |
| `src/cli.rs` | `rustman run` command: argument parsing, cli/json/junit reporters |
| `src/exec.rs` | Running one request end to end: pre-request script → variable substitution → send → post-response script |
| `src/script.rs` | QuickJS script sandbox, passing variables/request/response as JSON, `pm.sendRequest` |
| `src/script/prelude.js` | The `pm` API (variables, request, response, `pm.test`, `pm.expect`, JSON schema, ...) written in JS |
| `src/script/require.js` | `require()` for scripts, Node-style modules (`uuid`, `buffer`, `querystring`, `util`, `xml2js`), `crypto.getRandomValues` |
| `src/script/lib/` | Bundled JS libraries (crypto-js, lodash, moment, tv4, ajv, chai) with `LICENSES.md` |
| `src/import.rs` | Postman import/export, cURL parsing |
| `src/openapi.rs` | OpenAPI 3 / Swagger 2 parsing, collection generation |
| `src/mock.rs` | Mock HTTP server (tokio) |
| `src/dataset.rs` | CSV/JSON reading and writing for datasets |
| `src/mcp.rs` | MCP server for toolsets (JSON-RPC: initialize, tools/list, tools/call) |
| `src/codegen.rs` | Code snippet generation for 34 languages + syntax highlighter |
| `src/storage.rs` | Workspace load/save |
| `src/theme.rs` | Light/dark palettes (measured from Postman), fonts, egui style |
| `src/pm.rs` | Hand-drawn Postman-style widgets: tabs, checkbox, radio, toggle, key/value table, code viewer |
| `src/icons.rs` | Phosphor icon codepoints |
| `src/app.rs` | Overall state, logic and layout |
| `src/app/chrome.rs` | Menu bar, header + search, tab bar, status bar, right panel (Code / Info) |
| `src/app/sidebar.rs` | Sidebar: collection tree, environments, specs, mocks, datasets, documents, history |
| `src/app/request.rs` | Breadcrumb, URL bar, Params/Auth/Headers/Body/Settings |
| `src/app/response.rs` | Response pane |
| `src/app/dialogs.rs` | Dialogs |
| `src/app/oauth.rs` | OAuth 2.0 token flow and the waiting-for-login dialog |
| `src/app/environment.rs` | Environment / globals tab |
| `src/app/console.rs` | Console (request log) |
| `src/app/mocks.rs` | Mock server tab |
| `src/app/runner.rs` | Collection runner |
| `src/app/toolsets.rs` | Toolset tab (tool selection, parameters, running the MCP server) |
| `src/app/terminal.rs` | Integrated terminal (portable-pty + vt100) |
| `src/app/views.rs` | Spec, dataset and document tabs; shared header; Markdown renderer |

## License

Rustman is released under the [MIT](LICENSE) license.

Fonts: Inter, IBM Plex Mono (SIL OFL 1.1); icons: Phosphor (MIT). License files are in `assets/fonts/`. Bundled JS libraries: see `src/script/lib/LICENSES.md`.

## Tests

```bash
cargo test
# End-to-end tests against a local echo server (optional):
RUSTMAN_TEST_URL=http://127.0.0.1:18765 cargo test
```
