# Rust migration plan: ARTEX (Go)

1 packages, 217 source files (70506 lines), 265 test files, 0 UI files, grouped into 17 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 3 | 14 | 0 | 0 |

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `config/` | 212 | 62 | 81 | 0 | cgo | Pointers and manual memory (1); Null references (17); JSON round-trips (1) |
| 2 | `report/` | 290 | 59 | 51 | 0 | cgo | Pointers and manual memory (54); String offsets and slicing (1); Regex features the `regex` crate lacks (1) |
| 3 | `egress/` | 414 | 46 | 33 | 2 | cgo | Pointers and manual memory (40); Null references (34); Concurrency (14) |
| 4 | `mcphttp/` | 585 | 33 | 16 | 0 | cgo | Pointers and manual memory (45); String offsets and slicing (1); Null references (92) |
| 5 | `intercept/` | 3863 | 28 | 0 | 28 | cgo | Pointers and manual memory (177); String offsets and slicing (36); Regex features the `regex` crate lacks (2) |
| 6 | `agent/` | 9561 | 28 | 0 | 26 | cgo | Pointers and manual memory (482); String offsets and slicing (22); Regex features the `regex` crate lacks (2) |
| 7 | `db/` | 21160 | 26 | 0 | 97 | cgo | Pointers and manual memory (2566); String offsets and slicing (33); Regex features the `regex` crate lacks (2) |
| 8 | `notify/` | 2733 | 25 | 0 | 2 | cgo | Pointers and manual memory (51); String offsets and slicing (19); Null references (118) |
| 9 | `server/` | 26397 | 19 | 0 | 0 | cgo | Pointers and manual memory (1853); String offsets and slicing (49); Regex features the `regex` crate lacks (6) |

### `config/`

212 lines in 1 files, wave 0, priority 62, readiness 81, bridge **cgo**.

- Leaf: imports no other unit.
- Pointers and manual memory x1: `config/config.go:127`
- Null references x17: `config/config.go:47`, `config/config.go:64`, `config/config.go:77`
- JSON round-trips x1: `config/config.go:6`

### `report/`

290 lines in 2 files, wave 2, priority 59, readiness 51, bridge **cgo**.

- Pointers and manual memory x54: `report/findings.go:21`, `report/findings.go:32`, `report/findings.go:38`
- String offsets and slicing x1: `report/findings.go:139`
- Regex features the `regex` crate lacks x1: `report/findings.go:8`
- Null references x4: `report/findings.go:39`, `report/findings.go:147`, `report/report.go:95`
- Callback-shaped APIs x2: `report/findings.go:22`, `report/report.go:85`
- Integer width and overflow x2: `report/findings.go:39`, `report/findings.go:147`
- JSON round-trips x1: `report/report.go:6`

### `egress/`

414 lines in 1 files, wave 3, priority 46, readiness 33, bridge **cgo**.

- 2 test files import it directly.
- Pointers and manual memory x40: `egress/egress.go:79`, `egress/egress.go:99`, `egress/egress.go:100`
- Null references x34: `egress/egress.go:145`, `egress/egress.go:160`, `egress/egress.go:162`
- Concurrency x14: `egress/egress.go:24`, `egress/egress.go:83`, `egress/egress.go:97`
- Callback-shaped APIs x9: `egress/egress.go:79`, `egress/egress.go:83`, `egress/egress.go:155`
- Integer width and overflow x8: `egress/egress.go:136`, `egress/egress.go:146`, `egress/egress.go:189`
- Division and modulo x2: `egress/egress.go:136`, `egress/egress.go:191`
- Dynamic typing x1: `egress/egress.go:390`
- Exceptions x5: `egress/egress.go:143`, `egress/egress.go:187`, `egress/egress.go:216`

### `mcphttp/`

585 lines in 1 files, wave 0, priority 33, readiness 16, bridge **cgo**.

- 3 Go modules dependencies not in the crate table: check them by hand.
- Leaf: imports no other unit.
- Pointers and manual memory x45: `mcphttp/client.go:48`, `mcphttp/client.go:56`, `mcphttp/client.go:64`
- String offsets and slicing x1: `mcphttp/client.go:604`
- Null references x92: `mcphttp/client.go:93`, `mcphttp/client.go:97`, `mcphttp/client.go:109`
- Concurrency x15: `mcphttp/client.go:23`, `mcphttp/client.go:80`, `mcphttp/client.go:83`
- Callback-shaped APIs x4: `mcphttp/client.go:333`, `mcphttp/client.go:336`, `mcphttp/client.go:538`
- Integer width and overflow x2: `mcphttp/client.go:241`, `mcphttp/client.go:587`
- Exceptions x6: `mcphttp/client.go:161`, `mcphttp/client.go:224`, `mcphttp/client.go:493`
- JSON round-trips x1: `mcphttp/client.go:16`

### `intercept/`

3863 lines in 16 files, wave 2, priority 28, readiness 0, bridge **cgo**.

- 2 Go modules dependencies not in the crate table: check them by hand.
- 28 test files import it directly.
- Pointers and manual memory x177: `intercept/alwaysallow_denylist.go:89`, `intercept/argv.go:83`, `intercept/argv.go:96`
- String offsets and slicing x36: `intercept/alwaysallow_denylist.go:59`, `intercept/argv.go:30`, `intercept/argv.go:38`
- Regex features the `regex` crate lacks x2: `intercept/intercept.go:20`, `intercept/target_extract.go:7`
- Hash iteration order x1: `intercept/alwaysallow_denylist.go:18`
- Null references x278: `intercept/alwaysallow_denylist.go:76`, `intercept/alwaysallow_denylist.go:84`, `intercept/alwaysallow_denylist.go:89`
- Callback-shaped APIs x40: `intercept/alwaysallow_denylist.go:16`, `intercept/argv.go:94`, `intercept/intercept.go:60`
- Integer width and overflow x22: `intercept/argv.go:208`, `intercept/closed_judge.go:52`, `intercept/closed_judge.go:119`
- Dynamic typing x18: `intercept/argv.go:83`, `intercept/argv.go:95`, `intercept/argv.go:123`
- Concurrency x18: `intercept/intercept.go:23`, `intercept/intercept.go:147`, `intercept/intercept.go:150`
- Exceptions x18: `intercept/intercept.go:175`, `intercept/intercept.go:327`, `intercept/intercept.go:350`
- JSON round-trips x13: `intercept/alwaysallow_denylist.go:4`, `intercept/argv.go:4`, `intercept/closed_judge.go:6`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. github.com/Autumn-27/artex (`.`, portable, 265 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `config/` | ready | 212 | 81 | cgo | Pointers and manual memory (1); Null references (17) |
| 0 | `llmpool/` | caution | 444 | 30 | cgo | Pointers and manual memory (34); String offsets and slicing (3) |
| 0 | `mcphttp/` | caution | 585 | 16 | cgo | Pointers and manual memory (45); String offsets and slicing (1) |
| 0 | `sidequestion/` | caution | 704 | 7 | cgo | Pointers and manual memory (20); String offsets and slicing (12) |
| 0 | `notify/` | caution | 2733 | 0 | cgo | Pointers and manual memory (51); String offsets and slicing (19) |
| 1 | `db/` | caution | 21160 | 0 | cgo | Pointers and manual memory (2566); String offsets and slicing (33) |
| 2 | `report/` | ready | 290 | 51 | cgo | Pointers and manual memory (54); String offsets and slicing (1) |
| 2 | `llmrec/` | caution | 523 | 25 | cgo | Pointers and manual memory (31); String offsets and slicing (5) |
| 2 | `intercept/` | caution | 3863 | 0 | cgo | Pointers and manual memory (177); String offsets and slicing (36) |
| 2 | `traffic/` | caution | 2480 | 0 | cgo | Pointers and manual memory (180); String offsets and slicing (14) |
| 3 | `guard/` | caution | 330 | 50 | cgo | Pointers and manual memory (30); String offsets and slicing (2) |
| 3 | `evidence/` | caution | 399 | 48 | cgo | Pointers and manual memory (33); String offsets and slicing (1) |
| 3 | `egress/` | caution | 414 | 33 | cgo | Pointers and manual memory (40); Null references (34) |
| 4 | `enrich/` | caution | 280 | 37 | cgo | Pointers and manual memory (24); String offsets and slicing (1) |
| 4 | `agent/` | caution | 9561 | 0 | cgo | Pointers and manual memory (482); String offsets and slicing (22) |
| 5 | `server/` | caution | 26397 | 0 | cgo | Pointers and manual memory (1853); String offsets and slicing (49) |
| 6 | `cmd/artex/` | ready | 131 | 81 | cgo | Pointers and manual memory (1); Null references (7) |

## Go modules dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `github.com/Autumn-27/norma/llm` | 6 | ? | unknown |  |
| `github.com/Autumn-27/norma/permission` | 4 | ? | unknown |  |
| `github.com/Autumn-27/norma/tool` | 4 | ? | unknown |  |
| `github.com/Autumn-27/norma/harness` | 3 | ? | unknown |  |
| `github.com/Autumn-27/norma/transcript` | 3 | ? | unknown |  |
| `github.com/Autumn-27/norma/compaction` | 2 | ? | unknown |  |
| `github.com/jackc/pgx/v5/pgconn` | 2 | ? | unknown |  |
| `golang.org/x/net/idna` | 2 | ? | unknown |  |
| `github.com/Autumn-27/norma/agentcore` | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/hook` | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/mcp` | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/noaadapter` | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/skill` | 1 | ? | unknown |  |
| `github.com/golang-jwt/jwt/v5` | 1 | ? | unknown |  |
| `github.com/jackc/pgx/v5/stdlib` | 1 | ? | unknown |  |
| `github.com/klauspost/compress/zstd` | 1 | ? | unknown |  |
| `github.com/lqqyt2423/go-mitmproxy/proxy` | 1 | ? | unknown |  |
| `github.com/miekg/dns` | 1 | ? | unknown |  |
| `github.com/projectdiscovery/dnsx/libs/dnsx` | 1 | ? | unknown |  |
| `golang.org/x/crypto/bcrypt` | 1 | ? | unknown |  |
| `golang.org/x/net/publicsuffix` | 1 | ? | unknown |  |
| `golang.org/x/text/encoding/simplifiedchinese` | 1 | ? | unknown |  |
| `modernc.org/sqlite` | 1 | ? | unknown |  |
| `mvdan.cc/sh/v3/syntax` | 1 | ? | unknown |  |
| `github.com/google/uuid` | 1 | uuid | good |  |

## Risk guide

- **String offsets and slicing** (High): JS indexes strings in UTF-16 code units, Rust in UTF-8 bytes. Offsets that cross the boundary must be converted, or non-ASCII text gets wrong ranges silently.
- **Regex features the `regex` crate lacks** (High): Lookaround, backreferences and the `y` flag are not in the `regex` crate. Use `fancy-regex` for those patterns, or rewrite them, and test them on non-ASCII text: `\w`, `\b` and case-insensitive matching differ between engines even where a pattern compiles. JS `/i` without `u` never matches a non-ASCII letter to an ASCII one, Rust `(?i)` does (`ſ` matches `s`, the Kelvin sign `K` matches `k`): write `(?i-u)` or compare with `eq_ignore_ascii_case`.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Exceptions** (Low): Thrown errors become `Result`. Errors crossing the bridge must map back to the same JS error shape.
- **Callback-shaped APIs** (Medium): Function-typed parameters and methods cross the bridge on every call. Keep boundaries coarse: pass data in, get data out.
- **JSON round-trips** (Low): JSON.stringify key order and number formatting must match `serde_json` output if the result is user-visible.
- **Integer width and overflow** (Medium): Python ints never overflow; C, Go and Java ints wrap or are undefined on overflow. Pick i64, i128 or a bignum crate deliberately, and use wrapping_* where the source wraps.
- **Division and modulo** (Medium): Python `//` and `%` floor toward negative infinity; Rust `/` and `%` truncate: use `div_euclid`/`rem_euclid` or explicit floor math. C division by zero is undefined.
- **Hash iteration order** (High): Set and map iteration order comes from the source language's hash function. A port cannot reproduce it: sort before output, or make the order part of the contract.
- **Pointers and manual memory** (High): Pointer arithmetic and manual memory become slices, indices and owned types. Code that exposes pointers across the boundary needs `unsafe`, isolated and explained.
- **Concurrency** (Medium): Threads, goroutines and locks need a design in Rust (channels, Send/Sync), not a line-by-line translation; keep concurrency in the caller where possible.
- **Null references** (Medium): Any reference may be null in the source: decide per parameter whether it is `Option` and what null means, instead of mapping every value to `Option`.

## Whole app

Shape: **server**.

| Entry point | Kind | Found from |
| --- | --- | --- |
| artex: `cmd/artex` | cli | a `cmd/` folder |
| `cmd/artex/main.go` | server | `ListenAndServe` at cmd/artex/main.go:121 |

### Every dependency

| Dependency | Declared | Used by | Rust | Fit | Note |
| --- | --- | --- | --- | --- | --- |
| `github.com/Autumn-27/norma/llm` | no | 6 | ? | unknown |  |
| `github.com/Autumn-27/norma/permission` | no | 4 | ? | unknown |  |
| `github.com/Autumn-27/norma/tool` | no | 4 | ? | unknown |  |
| `github.com/Autumn-27/norma/harness` | no | 3 | ? | unknown |  |
| `github.com/Autumn-27/norma/transcript` | no | 3 | ? | unknown |  |
| `github.com/Autumn-27/norma/compaction` | no | 2 | ? | unknown |  |
| `github.com/jackc/pgx/v5/pgconn` | no | 2 | ? | unknown |  |
| `golang.org/x/net/idna` | no | 2 | ? | unknown |  |
| `github.com/Autumn-27/norma/agentcore` | no | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/hook` | no | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/mcp` | no | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/noaadapter` | no | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma/skill` | no | 1 | ? | unknown |  |
| `github.com/golang-jwt/jwt/v5` | yes | 1 | ? | unknown |  |
| `github.com/jackc/pgx/v5/stdlib` | no | 1 | ? | unknown |  |
| `github.com/klauspost/compress/zstd` | no | 1 | ? | unknown |  |
| `github.com/lqqyt2423/go-mitmproxy/proxy` | no | 1 | ? | unknown |  |
| `github.com/miekg/dns` | yes | 1 | ? | unknown |  |
| `github.com/projectdiscovery/dnsx/libs/dnsx` | no | 1 | ? | unknown |  |
| `golang.org/x/crypto/bcrypt` | no | 1 | ? | unknown |  |
| `golang.org/x/net/publicsuffix` | no | 1 | ? | unknown |  |
| `golang.org/x/text/encoding/simplifiedchinese` | no | 1 | ? | unknown |  |
| `modernc.org/sqlite` | yes | 1 | ? | unknown |  |
| `mvdan.cc/sh/v3/syntax` | no | 1 | ? | unknown |  |
| `github.com/Autumn-27/norma` | yes | 0 | ? | unknown |  |
| `github.com/klauspost/compress` | yes | 0 | ? | unknown |  |
| `github.com/lqqyt2423/go-mitmproxy` | yes | 0 | ? | unknown |  |
| `github.com/projectdiscovery/dnsx` | yes | 0 | ? | unknown |  |
| `golang.org/x/crypto` | yes | 0 | ? | unknown |  |
| `golang.org/x/net` | yes | 0 | ? | unknown |  |
| `mvdan.cc/sh/v3` | yes | 0 | ? | unknown |  |
| `golang.org/x/text` | yes | 0 | unicode-normalization, icu | partial |  |
| `github.com/google/uuid` | yes | 1 | uuid | good |  |
| `github.com/jackc/pgx/v5` | yes | 0 | sqlx, tokio-postgres | good |  |

### Pieces, in port order

- wave 0: `config/` (212 lines)
- wave 0: `llmpool/` (444 lines)
- wave 0: `mcphttp/` (585 lines)
- wave 0: `notify/` (2733 lines)
- wave 0: `sidequestion/` (704 lines)
- wave 1: `db/` (21160 lines)
- wave 2: `intercept/` (3863 lines)
- wave 2: `llmrec/` (523 lines)
- wave 2: `report/` (290 lines)
- wave 2: `traffic/` (2480 lines)
- wave 3: `egress/` (414 lines)
- wave 3: `evidence/` (399 lines)
- wave 3: `guard/` (330 lines)
- wave 4: `agent/` (9561 lines)
- wave 4: `enrich/` (280 lines)
- wave 5: `server/` (26397 lines)
- wave 6: `cmd/artex/` (131 lines)

### Decisions for the user

**D1** (shape): The app is a server, entered through artex (cmd/artex), cmd/artex/main.go. Port every entry point, in that shape?

> Yes: an axum front door (`app front-door`) takes the server over first

**D2** (dependency github.com/Autumn-27/norma/llm): Which crate replaces github.com/Autumn-27/norma/llm (used by 6 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-llm.md before porting what uses it

**D3** (dependency github.com/Autumn-27/norma/permission): Which crate replaces github.com/Autumn-27/norma/permission (used by 4 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-permission.md before porting what uses it

**D4** (dependency github.com/Autumn-27/norma/tool): Which crate replaces github.com/Autumn-27/norma/tool (used by 4 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-tool.md before porting what uses it

**D5** (dependency github.com/Autumn-27/norma/harness): Which crate replaces github.com/Autumn-27/norma/harness (used by 3 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-harness.md before porting what uses it

**D6** (dependency github.com/Autumn-27/norma/transcript): Which crate replaces github.com/Autumn-27/norma/transcript (used by 3 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-transcript.md before porting what uses it

**D7** (dependency github.com/Autumn-27/norma/compaction): Which crate replaces github.com/Autumn-27/norma/compaction (used by 2 units)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-compaction.md before porting what uses it

**D8** (dependency github.com/jackc/pgx/v5/pgconn): Which crate replaces github.com/jackc/pgx/v5/pgconn (used by 2 units)?

> Research it against primary sources into rust/research/github.com-jackc-pgx-v5-pgconn.md before porting what uses it

**D9** (dependency golang.org/x/net/idna): Which crate replaces golang.org/x/net/idna (used by 2 units)?

> Research it against primary sources into rust/research/golang.org-x-net-idna.md before porting what uses it

**D10** (dependency github.com/Autumn-27/norma/agentcore): Which crate replaces github.com/Autumn-27/norma/agentcore (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-agentcore.md before porting what uses it

**D11** (dependency github.com/Autumn-27/norma/hook): Which crate replaces github.com/Autumn-27/norma/hook (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-hook.md before porting what uses it

**D12** (dependency github.com/Autumn-27/norma/mcp): Which crate replaces github.com/Autumn-27/norma/mcp (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-mcp.md before porting what uses it

**D13** (dependency github.com/Autumn-27/norma/noaadapter): Which crate replaces github.com/Autumn-27/norma/noaadapter (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-noaadapter.md before porting what uses it

**D14** (dependency github.com/Autumn-27/norma/skill): Which crate replaces github.com/Autumn-27/norma/skill (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma-skill.md before porting what uses it

**D15** (dependency github.com/golang-jwt/jwt/v5): Which crate replaces github.com/golang-jwt/jwt/v5 (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-golang-jwt-jwt-v5.md before porting what uses it

**D16** (dependency github.com/jackc/pgx/v5/stdlib): Which crate replaces github.com/jackc/pgx/v5/stdlib (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-jackc-pgx-v5-stdlib.md before porting what uses it

**D17** (dependency github.com/klauspost/compress/zstd): Which crate replaces github.com/klauspost/compress/zstd (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-klauspost-compress-zstd.md before porting what uses it

**D18** (dependency github.com/lqqyt2423/go-mitmproxy/proxy): Which crate replaces github.com/lqqyt2423/go-mitmproxy/proxy (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-lqqyt2423-go-mitmproxy-proxy.md before porting what uses it

**D19** (dependency github.com/miekg/dns): Which crate replaces github.com/miekg/dns (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-miekg-dns.md before porting what uses it

**D20** (dependency github.com/projectdiscovery/dnsx/libs/dnsx): Which crate replaces github.com/projectdiscovery/dnsx/libs/dnsx (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-projectdiscovery-dnsx-libs-dnsx.md before porting what uses it

**D21** (dependency golang.org/x/crypto/bcrypt): Which crate replaces golang.org/x/crypto/bcrypt (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-crypto-bcrypt.md before porting what uses it

**D22** (dependency golang.org/x/net/publicsuffix): Which crate replaces golang.org/x/net/publicsuffix (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-net-publicsuffix.md before porting what uses it

**D23** (dependency golang.org/x/text/encoding/simplifiedchinese): Which crate replaces golang.org/x/text/encoding/simplifiedchinese (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-text-encoding-simplifiedchinese.md before porting what uses it

**D24** (dependency modernc.org/sqlite): Which crate replaces modernc.org/sqlite (used by 1 unit)?

> Research it against primary sources into rust/research/modernc.org-sqlite.md before porting what uses it

**D25** (dependency mvdan.cc/sh/v3/syntax): Which crate replaces mvdan.cc/sh/v3/syntax (used by 1 unit)?

> Research it against primary sources into rust/research/mvdan.cc-sh-v3-syntax.md before porting what uses it

**D26** (dependency github.com/Autumn-27/norma): Which crate replaces github.com/Autumn-27/norma (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-Autumn-27-norma.md before porting what uses it

**D27** (dependency github.com/klauspost/compress): Which crate replaces github.com/klauspost/compress (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-klauspost-compress.md before porting what uses it

**D28** (dependency github.com/lqqyt2423/go-mitmproxy): Which crate replaces github.com/lqqyt2423/go-mitmproxy (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-lqqyt2423-go-mitmproxy.md before porting what uses it

**D29** (dependency github.com/projectdiscovery/dnsx): Which crate replaces github.com/projectdiscovery/dnsx (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-projectdiscovery-dnsx.md before porting what uses it

**D30** (dependency golang.org/x/crypto): Which crate replaces golang.org/x/crypto (declared, no unit imports it)?

> Research it against primary sources into rust/research/golang.org-x-crypto.md before porting what uses it

**D31** (dependency golang.org/x/net): Which crate replaces golang.org/x/net (declared, no unit imports it)?

> Research it against primary sources into rust/research/golang.org-x-net.md before porting what uses it

**D32** (dependency mvdan.cc/sh/v3): Which crate replaces mvdan.cc/sh/v3 (declared, no unit imports it)?

> Research it against primary sources into rust/research/mvdan.cc-sh-v3.md before porting what uses it

**D33** (dependency golang.org/x/text): golang.org/x/text maps to unicode-normalization, icu only in part (declared, no unit imports it). Use it?

> Use unicode-normalization, icu, and record cases for the behavior it does not cover

