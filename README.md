<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/logo-dark.svg">
    <img src="assets/logo/logo.svg" alt="Rustman" width="128" height="128">
  </picture>
</p>

# Rustman

[English](README.en.md) | **Tiếng Việt**

[![CI](https://github.com/nhamtrieu/rustman/actions/workflows/ci.yml/badge.svg)](https://github.com/nhamtrieu/rustman/actions/workflows/ci.yml)

API client kiểu Postman, viết bằng Rust (egui + reqwest). Native, một file binary, không cần tài khoản hay cloud.

## Tải về

Trang [Releases](https://github.com/nhamtrieu/rustman/releases) có hai loại bản:

- **Bản ổn định** (`v0.2.0`, ...): phát hành khi gắn tag phiên bản.
- **Nightly**: tự build lại sau mỗi lần push lên `main`, tên file không đổi nên link tải luôn trỏ tới bản mới nhất, ví dụ [rustman-nightly-amd64.deb](https://github.com/nhamtrieu/rustman/releases/download/nightly/rustman-nightly-amd64.deb). Có thể chưa ổn định.

| Hệ điều hành | Trình cài đặt | File nén |
|---|---|---|
| Ubuntu / Debian (x86_64) | `rustman-…-amd64.deb`: `sudo apt install ./rustman-…-amd64.deb`, có sẵn icon và mục trong menu ứng dụng | `…-x86_64-unknown-linux-gnu.tar.gz`; icon dock: `scripts/install-desktop.sh ./rustman` |
| macOS Apple Silicon / Intel | `rustman-…-macos-arm64.dmg` / `…-macos-x64.dmg`: kéo Rustman vào Applications | `…-aarch64-apple-darwin.tar.gz` / `…-x86_64-apple-darwin.tar.gz` |
| Windows (x86_64) | `rustman-…-x64.msi`: cài vào Program Files, có shortcut trong Start Menu | `…-x86_64-pc-windows-msvc.zip` |

Mỗi bản có `SHA256SUMS.txt` để kiểm tra file. Bản macOS chưa được Apple ký: lần đầu mở, chuột phải vào Rustman → Open (hoặc `xattr -d com.apple.quarantine /Applications/Rustman.app`). Windows SmartScreen có thể cảnh báo vì trình cài chưa ký: chọn *More info* → *Run anyway*.

## Chạy từ source

```bash
cargo run --release
```

Trên Linux cần các thư viện hệ thống cho winit/wgpu (X11/Wayland, xkbcommon). Hộp thoại chọn file dùng xdg-desktop-portal.

Để dock/taskbar trên Linux (GNOME, KDE...) hiện đúng icon và có Rustman trong menu ứng dụng, cài launcher cho user hiện tại sau khi build:

```bash
scripts/install-desktop.sh            # dùng target/release/rustman
scripts/install-desktop.sh --uninstall
```

## Tính năng

- **Request**: GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS, query params đồng bộ hai chiều với URL, headers, nhiều tab (chuột phải vào tab: New Request, Duplicate Tab, Close / Force Close Tab, Close Other Tabs, Close All Tabs, Force Close All Tabs).
- **Body**: raw (JSON có tô màu, kiểm tra lỗi và Beautify; Text/XML/HTML/JS), x-www-form-urlencoded, form-data (text + file), binary, GraphQL.
- **Auth**: Bearer, Basic, API Key (header/query), Digest (tự trả lời challenge 401; MD5, SHA-256, `-sess`), OAuth 2.0, AWS Signature v4 (kể cả session token cho credentials tạm thời), "Inherit auth from parent" lấy auth của folder gần nhất hoặc của collection.
- **OAuth 2.0** (request hoặc collection): grant Authorization Code, Authorization Code (PKCE, SHA-256/plain), Implicit, Password Credentials, Client Credentials. Nút *Get New Access Token* mở trình duyệt; nếu Callback URL là `http://localhost:<port>` hoặc `127.0.0.1` thì Rustman tự bắt redirect, nếu không thì dán URL trình duyệt chuyển tới vào hộp thoại. Gửi client credentials bằng Basic header hoặc trong body; có Scope, State, Audience, Resource, Refresh Token URL. Token gắn vào header (prefix tuỳ chọn) hoặc query `access_token`; hiện thời hạn, nút *Refresh*/*Clear*, và *Auto-refresh*: token hết hạn được refresh trước khi gửi (cả trong Collection Runner). Request lấy token hiện trong Console. `{{biến}}` dùng được ở mọi ô.
- **Settings** cho từng request: timeout, follow redirects, verify SSL.
- **Proxy** (Settings → Proxy): dùng proxy hệ thống (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`) hoặc proxy tuỳ chỉnh (host:port, chọn áp dụng cho HTTP và/hoặc HTTPS, Basic auth, danh sách bypass: tên miền, `*.domain`, IP, CIDR; mặc định bỏ qua localhost). Áp dụng cho mọi request: tab, runner, `pm.sendRequest`, OAuth 2.0, MCP.
- **Certificates** (Settings → Certificates): thêm CA (file PEM, tin cậy thêm ngoài CA có sẵn) và client certificate cho mutual TLS theo host (`api.example.com`, `*.example.com`, có thể kèm `:port`), file CRT + KEY dạng PEM.
- **Scripts** (tab *Scripts* của request, folder và collection): JavaScript chạy *Pre-request* (trước khi gửi) và *Post-response* (sau khi nhận response), API `pm` tương thích Postman. Thứ tự chạy: script của collection, rồi của từng folder chứa request (từ ngoài vào trong), rồi của request. Chi tiết ở mục [Scripts](#scripts).
- **Test Results**: kết quả `pm.test` (PASS/FAIL/SKIPPED, lọc theo trạng thái) và lỗi script kèm số dòng.
- **Response**: status/time/size, JSON pretty có tô màu (hiển thị ảo hoá theo dòng, xem được response lớn), Raw, Headers, Cookies, Info, Copy, Save ra file.
- **Collections**: folder lồng nhau, kéo thả để sắp xếp/di chuyển, rename/duplicate/delete, biến, auth và script cấp collection. Folder có mô tả, auth và script riêng (chuột phải folder → *Edit* / *Authorization* / *Scripts*); auth của folder đặt *Inherit* thì lấy tiếp từ cấp trên.
- **Environments & biến**: `{{var}}` ở mọi chỗ, độ ưu tiên environment > collection > globals; biến động `{{$guid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`, `{{$randomEmail}}`...; tô xanh/đỏ cho biến đã/chưa định nghĩa, rê chuột để xem giá trị đã thay.
- **Console** (status bar → Console, hoặc Ctrl+Alt+C): log mọi request đã gửi với URL đã thay biến, request/response headers và body, thời gian, kích thước, lỗi; lọc theo chữ hoặc chỉ lỗi; chuột phải để copy URL/response.
- **Toolsets (MCP)**: chọn các request đã lưu làm *tool* cho AI agent (Claude Code, Cursor, VS Code…). Rustman chạy một MCP server cục bộ ở `http://localhost:<port>/mcp` (Streamable HTTP). Các `{{biến}}` của request mà agent cần điền trở thành tham số của tool (có mô tả, bắt buộc/không); các biến còn lại lấy giá trị từ environment / collection đang dùng. Nút *Connect* có sẵn lệnh `claude mcp add --transport http …` và JSON config. Có log các lần agent gọi tool.
- **Terminal** (status bar → Terminal, hoặc Ctrl+\`): shell thật trong PTY (bash/zsh/PowerShell) nằm chung panel với Console. Hỗ trợ màu 16/256/truecolor, phím mũi tên, Ctrl+C/D/Z…, dán (bracketed paste), cuộn lại lịch sử bằng chuột, chương trình toàn màn hình (vim, htop). Ctrl+Shift+C hoặc nút copy để chép màn hình.
- **Cookie jar** dùng chung giữa các request (Tools → Clear cookies để xoá).
- **History** của các request đã gửi.
- **Import**: Postman Collection v2.1 (kể cả script, auth và mô tả của folder), Postman Environment, OpenAPI 3 / Swagger 2 (JSON hoặc YAML, tạo cả spec lẫn collection), lệnh cURL (dán thẳng vào ô URL cũng được).
- **Specs**: soạn OpenAPI (YAML/JSON) với danh sách endpoint và báo lỗi cú pháp ngay bên cạnh; nút *Generate Collection* sinh collection (folder theo tag, biến `{{baseUrl}}`, path param thành biến, query/header, body mẫu từ schema).
- **Mocks**: mock server chạy cục bộ (`http://localhost:<port>`). Mỗi route có method (hoặc ANY), path dạng `/users/:id` hay `/files/*`, status, headers, body, delay. Body/header dùng được `{{id}}` (path param), query param và biến động `{{$guid}}`. Mở CORS sẵn, có log các request gọi tới. Đổi route khi đang chạy thì có hiệu lực ngay.
- **Datasets**: bảng dữ liệu sửa trực tiếp, import CSV/JSON, export CSV. Tên cột là tên biến.
- **Collection Runner** (chuột phải collection → *Run collection*): chọn request, số lượt, delay, dataset (lượt n dùng dòng n, cột ghi đè các biến khác). Request chạy tuần tự kèm script: biến do request trước đặt được request sau dùng, `pm.execution.setNextRequest` đổi thứ tự. Một request *Passed* khi mọi test pass (không có test thì khi status < 400); kết quả từng test hiện dưới request. Mọi request và `console.log` cũng được ghi vào Console.
- **Documents**: ghi chú Markdown (tiêu đề, danh sách, checklist, code block, **đậm**, *nghiêng*, `code`) với chế độ Edit/Preview.
- **Export**: Postman Collection v2.1 (kể cả script và auth của folder), Postman Environment.
- **Code snippet** (panel bên phải, có tô màu cú pháp): 34 ngôn ngữ/thư viện — C libcurl, C# HttpClient/RestSharp, cURL, Dart dio/http, Go, HTTP, Java OkHttp/Unirest, JavaScript Fetch/jQuery/XHR, Kotlin OkHttp, NodeJs Axios/Native/Request/Unirest, Objective-C, OCaml, PHP cURL/Guzzle/HTTP_Request2/pecl_http, PowerShell, Python http.client/Requests, R httr/RCurl, Ruby, Rust reqwest, HTTPie, wget, Swift. Tuỳ chọn: thụt lề, follow redirect, dạng option cURL, ký tự nối dòng.
- Theme sáng/tối, tự lưu workspace.

## Chạy từ dòng lệnh (CI)

`rustman run` chạy collection không cần mở app, giống `newman run`:

```bash
rustman run collection.json -e dev.postman_environment.json -d users.csv \
  -r cli,junit --reporter-junit-export results.xml
```

- `<collection>`: file Postman Collection v2.1, hoặc tên một collection trong workspace của Rustman. Khi lấy từ workspace thì dùng luôn globals, proxy và chứng chỉ đã cài trong app.
- `-e` file environment hoặc tên environment trong workspace; `-g` file globals; `-d` file CSV/JSON (hoặc tên dataset), mỗi dòng một lượt; `-n` số lượt.
- `--folder <tên>` (lặp lại được) chỉ chạy folder/request đó; `--env-var k=v`, `--global-var k=v`; `--delay-request`, `--timeout-request`, `-k/--insecure`, `--bail`.
- Chứng chỉ: `--ssl-extra-ca-certs`, `--ssl-client-cert`, `--ssl-client-key`. Proxy lấy từ `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`.
- Report: `-r cli,json,junit`, `--reporter-json-export`, `--reporter-junit-export`; `--export-environment`, `--export-globals` ghi lại biến sau khi script chạy.
- Exit code: `0` khi mọi thứ pass, `1` khi có test/script/request lỗi (`-x` để luôn trả 0), `2` khi dùng sai lệnh. `rustman --help` để xem đủ.

## Scripts

Script là JavaScript (engine QuickJS, nhúng sẵn), mỗi script chạy trong một sandbox riêng, giới hạn 30 giây và 128 MB.

| API | Mô tả |
|---|---|
| `pm.environment`, `pm.collectionVariables`, `pm.globals` | `get`, `set`, `unset`, `has`, `clear`, `toObject`, `replaceIn`. Thay đổi được lưu vào workspace. |
| `pm.variables` | Đọc theo thứ tự ưu tiên local > data > environment > collection > globals; `set` chỉ có hiệu lực trong lần chạy request đó. `replaceIn("{{x}}")`. |
| `pm.iterationData` | Dòng dataset của lượt chạy hiện tại (chỉ đọc). |
| `pm.request` | `method`, `url` (`toString`, `getHost`, `getPath`, `query.add/upsert/remove`), `headers` (`get`, `add`, `upsert`, `remove`), `body.raw`. Sửa được trong Pre-request. |
| `pm.response` | `code`, `status`, `responseTime`, `responseSize`, `headers.get`, `cookies.get`, `text()`, `json()`. |
| `pm.test(name, fn)`, `pm.test.skip` | Khai báo test. |
| `pm.expect(x)` | Assertion kiểu chai: `equal`, `eql`, `deep`, `not`, `a/an`, `include`, `property`, `nested.property`, `lengthOf`, `above/below/least/most/within`, `keys`, `members`, `oneOf`, `match`, `ok/true/false/null/undefined/empty/exist`, `throw`, `jsonSchema`. |
| `pm.response.to...` | `have.status(200)`, `have.header(name[, value])`, `have.body(...)`, `have.jsonBody(path, value)`, `have.jsonSchema(schema)`, `be.ok/success/clientError/serverError/error/notFound/json`. |
| `pm.sendRequest(req[, callback])` | Gửi request phụ (URL hoặc `{url, method, header, body}`); dùng `await` hoặc callback `(err, res)`. Request phụ cũng hiện trong Console. |
| `pm.execution.setNextRequest(name \| null)` | Runner: nhảy tới request có tên đó, `null` để kết thúc lượt. |
| `pm.execution.skipRequest()` | Pre-request: không gửi request này. |
| `pm.info` | `requestName`, `requestId`, `iteration`, `iterationCount`, `eventName`. |
| `console.log/info/warn/error` | Ghi vào Console. |
| `require(tên)` | Thư viện có sẵn như sandbox Postman, chỉ nạp khi dùng: `crypto-js` 4.2, `lodash` 4.17, `moment` 2.30, `tv4`, `ajv` 6, `chai` 4 (bản gốc, giấy phép trong `src/script/lib/LICENSES.md`); `uuid`, `buffer`, `querystring`, `util`, `xml2js` (`parseString`, `Parser`, các option thường dùng), `atob`, `btoa`. |
| Global | `CryptoJS`, `_` (lodash), `tv4`, `xml2Json(xml)`, `Buffer`, `TextEncoder`/`TextDecoder`, `crypto.getRandomValues`, `atob`/`btoa`. |
| Cú pháp cũ | `tests["tên"] = điều_kiện`, `postman.setEnvironmentVariable`, `responseBody`, `responseCode`... |

Proxy/certificates chưa hỗ trợ: proxy SOCKS, file PFX/PKCS#12 và private key có mật khẩu (chuyển sang PEM bằng `openssl pkcs12 -in c.pfx -out c.pem -nodes`).

Chưa hỗ trợ: `cheerio`, `csv-parse`, `postman-collection`, module Node khác (`url`, `crypto`, ...), `pm.vault`. Toolsets (MCP) gửi request mà không chạy script.

## Phím tắt

| Phím | Tác dụng |
|---|---|
| Ctrl+Enter | Gửi request |
| Ctrl+S | Lưu request |
| Ctrl+T | Tab mới |
| Ctrl+W | Đóng tab |
| Ctrl+Alt+W | Đóng tab, bỏ thay đổi chưa lưu |
| Ctrl+O | Import |
| Shift+click / Ctrl+click (sidebar) | Chọn nhiều request / folder |
| Delete (sidebar) | Xoá các mục đang chọn (macOS: Cmd+Backspace) |
| Ctrl+\\ | Ẩn/hiện sidebar |
| Ctrl+Alt+C | Mở/đóng Console |
| Ctrl+\` | Mở/đóng Terminal |

## Dữ liệu

Workspace được lưu vào `~/.local/share/rustman/workspace.json` (Linux). Đặt `RUSTMAN_DATA_DIR=/đường/dẫn` để dùng thư mục khác (chế độ portable).

## Cấu trúc code

| File | Nội dung |
|---|---|
| `src/main.rs` | Điểm vào: chuyển sang `rustman run` (CLI) hoặc mở cửa sổ app |
| `src/model.rs` | Data model (Request, Collection, Environment, ...) |
| `src/vars.rs` | Thay thế biến `{{var}}` |
| `src/http.rs` | Dựng và gửi request (reqwest) |
| `src/oauth2.rs` | OAuth 2.0: các grant, PKCE, nhận callback trên localhost, refresh token |
| `src/run.rs` | Vòng lặp chạy collection (dùng chung cho tab Runner và `rustman run`) |
| `src/cli.rs` | Lệnh `rustman run`: đọc tham số, reporter cli/json/junit |
| `src/exec.rs` | Chạy một request trọn vẹn: pre-request script → thay biến → gửi → post-response script |
| `src/script.rs` | Sandbox QuickJS cho script, truyền biến/request/response qua JSON, `pm.sendRequest` |
| `src/script/prelude.js` | API `pm` (biến, request, response, `pm.test`, `pm.expect`, JSON schema, ...) viết bằng JS |
| `src/script/require.js` | `require()` trong script, các module kiểu Node (`uuid`, `buffer`, `querystring`, `util`, `xml2js`), `crypto.getRandomValues` |
| `src/script/lib/` | Thư viện JS nhúng sẵn (crypto-js, lodash, moment, tv4, ajv, chai) kèm `LICENSES.md` |
| `src/sign.rs` | Ký AWS Signature v4, tính header Digest; HMAC-SHA256 và MD5 |
| `src/import.rs` | Import/export Postman, parse cURL |
| `src/openapi.rs` | Đọc OpenAPI 3 / Swagger 2, sinh collection |
| `src/mock.rs` | Mock HTTP server (tokio) |
| `src/dataset.rs` | Đọc/ghi CSV, JSON cho dataset |
| `src/mcp.rs` | MCP server cho toolsets (JSON-RPC: initialize, tools/list, tools/call) |
| `src/codegen.rs` | Sinh code snippet cho 34 ngôn ngữ + bộ tô màu cú pháp |
| `src/storage.rs` | Lưu/đọc workspace |
| `src/theme.rs` | Bảng màu sáng/tối (đo từ Postman), font, style egui |
| `src/pm.rs` | Widget vẽ tay kiểu Postman: tab, checkbox, radio, toggle, bảng key/value, code viewer |
| `src/icons.rs` | Mã icon Phosphor |
| `src/app.rs` | State, logic, layout tổng |
| `src/app/chrome.rs` | Menu bar, header + search, tab bar, status bar, panel phải (Code / Info) |
| `src/app/sidebar.rs` | Sidebar: cây collection, environments, specs, mocks, datasets, documents, history |
| `src/app/request.rs` | Breadcrumb, ô URL, Params/Auth/Headers/Body/Settings |
| `src/app/response.rs` | Response pane |
| `src/app/dialogs.rs` | Các hộp thoại |
| `src/app/oauth.rs` | Chạy luồng lấy token OAuth 2.0 và hộp thoại chờ đăng nhập |
| `src/app/environment.rs` | Tab environment / globals |
| `src/app/console.rs` | Console (log request) |
| `src/app/mocks.rs` | Tab mock server |
| `src/app/runner.rs` | Collection runner |
| `src/app/toolsets.rs` | Tab toolset (chọn tool, tham số, chạy MCP server) |
| `src/app/terminal.rs` | Terminal tích hợp (portable-pty + vt100) |
| `src/app/views.rs` | Tab spec, dataset, document; header chung; renderer Markdown |

## License

Rustman phát hành theo giấy phép [MIT](LICENSE).

Font: Inter, IBM Plex Mono (SIL OFL 1.1); icon: Phosphor (MIT). File license nằm trong `assets/fonts/`. Thư viện JS nhúng sẵn: xem `src/script/lib/LICENSES.md`.

## Test

```bash
cargo test
# Test end-to-end với một echo server local (tuỳ chọn):
RUSTMAN_TEST_URL=http://127.0.0.1:18765 cargo test
```
