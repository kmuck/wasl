# 17. Standard library

*Specified, not available yet.*

The standard library is the package `wasl:std`, written in WaSL, shipped inside `weasel` and versioned with the compiler: `weasel` 0.5 ships `wasl:std@0.5.0`. Every project depends on it without declaring it (§ 3.5), and a file names one of its interfaces with `use` (§ 6.6):

```wasl
use wasl:std/text;
use wasl:std/json;
```

| Interface | Content | Uses |
|---|---|---|
| `text` | operations on strings (§ 17.2) | nothing |
| `json` | the JSON value, parsing and printing (§ 17.3) | nothing |
| `http` | an HTTP service: its request, its response, `handle` and a few helpers (§ 17.4) | `wasi:http` 0.3 |
| `cli` | a command: `run`, its arguments, its standard streams (§ 17.5) | `wasi:cli` 0.3 |
| `fs` | files as text or bytes (§ 17.6) | `wasi:filesystem` 0.3 |

It also declares two worlds, which a program includes (§ 17.1):

| World | Exports | Calls |
|---|---|---|
| `service` | `wasi:http/handler` | `http::handle`, written by the program |
| `command` | `wasi:cli/run` | `cli::run`, written by the program |

The library sits above WASI as the standard library of other languages sits above the system calls. It is a library like the others, which gets no special treatment from the compiler: its interfaces and its worlds follow the rules of the language (§ 2.4.3), its functions are compiled into the components that call them (§ 3.5), and what it does with WASI, a program can do too, by importing WASI itself.

The functions below are listed with their signatures; their bodies belong to the library. A parameter or result written `string` is UTF-8 text, as everywhere in WaSL.

## 17.1 Entry points

`wasl:std/http` and `wasl:std/cli` each declare one function without body (§ 2.4.3): `handle` and `run`. The worlds `wasl:std/service` and `wasl:std/command` export the entry point of WASI and call that function. A world of the program includes one of them, and writes the function:

```wasl
use wasl:std/cli;

world hello {
  include wasl:std/command {
    async func run() -> result<_, string> {
      print("Hello, world!\n");
      return result.ok;
    }
  }
}
```

The two worlds are written in WaSL, in the library:

```wasl
// in wasl:std, simplified
world command {
  export wasi:cli/run {
    async func run() -> result {
      match (cli::run()) {
        case ok {
          return result.ok;
        }
        case err(message) {
          cli::eprint("error: " + message + "\n");
          return result.err;
        }
      }
    }
  }
}

world service {
  export wasi:http/handler {
    async func handle(req: types::request) -> result<types::response, types::error_code> {
      local answer = http::handle(receive(req));
      local (response, body) = start(answer);
      task.return(result.ok(response));
      finish(body, answer);
    }
  }
}
```

**In the binary.** One component, which exports `wasi:cli/run` or `wasi:http/handler` and imports what the library and the program import from WASI. The exported function is the one of the library; it calls the function of the program, an internal function of the same module, with a plain `call` (§ 2.4.3). The WIT of the world says exactly this (§ 13.1), and wasmtime, or any other host, runs the file as it is.

```
build/hello.wasm, one component
┌──────────────────────────────────────────────────────────────┐
│  export wasi:cli/run (code of wasl:std) ──call──▶ run (hello) │
└──────────────────────────────────────────────────────────────┘
```

**Why the library writes the export.** `handle` of `wasi:http/handler` must deliver its response with `task.return` before it writes the body (§ 8.21), and works on streams, futures and resources. A function that returns a `response` record is simpler to write, and the library does the rest once for every program. The price is that the whole body is in memory, in both directions (§ 17.7). A program that wants the streams exports `wasi:http/handler` itself, as in 0.4.

### The world `service`

For each request, the code of `service`:

1. reads the method, the path, the query and the headers of the request of `wasi:http`; a header whose value is not valid UTF-8 is left out;
2. reads the whole body, up to **10 MiB**; a longer body gets the response `413` without calling `handle`;
3. calls `handle` with the `request` record (§ 17.4), and waits for its `response`;
4. delivers a response of `wasi:http` with the status, the headers in their order, and a `content-length` header unless the program gave one, then writes the body.

A trap in `handle` ends the request with the status `500` (§ 14.2). A status outside `100` to `599`, or a header that `wasi:http` refuses, is a trap.

### The world `command`

The code of `command` calls `run`. When it returns `result.ok`, the command exits with the code `0`. When it returns `result.err(message)`, the library writes `error: <message>` and a line feed on the standard error, and the command exits with the code `1`. A trap exits with the code `1` and the message of the host (§ 14.2).

## 17.2 `text`

```wasl
interface text {
  func split(s: string, separator: string) -> list<string> { … }
  func join(parts: list<string>, separator: string) -> string { … }
  func lines(s: string) -> list<string> { … }
  func trim(s: string) -> string { … }
  func starts_with(s: string, prefix: string) -> bool { … }
  func ends_with(s: string, suffix: string) -> bool { … }
  func contains(s: string, part: string) -> bool { … }
  func find(s: string, part: string) -> option<u32> { … }
  func replace(s: string, from: string, to: string) -> string { … }
  func slice(s: string, start: u32, end: u32) -> option<string> { … }
  func lower(s: string) -> string { … }
  func upper(s: string) -> string { … }
}
```

| Function | Result |
|---|---|
| `split(s, separator)` | the parts of `s` between the occurrences of `separator`, in order: `split("a,b,,c", ",")` is `["a", "b", "", "c"]`, `split("", ",")` is `[""]`. An empty separator gives `[s]`. |
| `join(parts, separator)` | the parts, with `separator` between each two: `join(["a", "b"], ", ")` is `"a, b"` |
| `lines(s)` | the lines of `s`: split on `"\n"`, without a `"\r"` that ends a line, and without the empty line after a final `"\n"`: `lines("a\r\nb\n")` is `["a", "b"]` |
| `trim(s)` | `s` without the spaces, tabs, carriage returns and line feeds at its start and end |
| `starts_with`, `ends_with`, `contains` | whether `s` starts with, ends with, or contains the bytes of the other string |
| `find(s, part)` | the position in bytes of the first occurrence of `part`, or `option.none` |
| `replace(s, from, to)` | `s` with every occurrence of `from`, from left to right, replaced by `to`; an empty `from` gives `s` |
| `slice(s, start, end)` | the bytes from `start` to `end` (excluded), or `option.none` if `start > end`, `end` is past the end, or a bound cuts a character in two |
| `lower(s)`, `upper(s)` | `s` with the ASCII letters in lowercase or uppercase; the other characters are unchanged |

Positions count bytes, as `string.len` does (§ 8.17): a position found by `find` can be given to `slice`. The functions compare bytes: `"é"` written with one code point and with two are different strings.

## 17.3 `json`

```wasl
interface json {
  variant value {
    null,
    boolean(bool),
    number(f64),
    text(string),
    array(list<value>),
    object(list<tuple<string, value>>),
  }

  func parse(s: string) -> result<value, string> { … }
  func stringify(v: value) -> string { … }

  func field(v: value, name: string) -> option<value> { … }
  func as_text(v: value) -> option<string> { … }
  func as_number(v: value) -> option<f64> { … }
  func as_u64(v: value) -> option<u64> { … }
  func as_bool(v: value) -> option<bool> { … }
  func as_array(v: value) -> option<list<value>> { … }
  func from_u64(n: u64) -> value { … }
}
```

- `value` is the JSON value of RFC 8259. It is a recursive type, so it never appears in the WIT (E0208): a function of an exported interface takes or returns the JSON **text**, a `string`, and converts it inside.
- `parse(s)` reads one JSON value, with whitespace around it, as RFC 8259 defines it. The fields of an object keep their order; a name that appears twice keeps both. A number becomes the nearest `f64`. Anything else is `result.err` with a message that gives the position in bytes: `"expected `:` at 12"`.
- `stringify(v)` writes `v` on one line, without spaces, with the fields in their order. A number that is an integer of at most 2⁵³ is written without decimal part (`3`, not `3.0`); NaN and the infinities, which JSON does not have, are written `null`. Strings are escaped as RFC 8259 requires, and the other characters are written as they are.
- `field(v, name)` is the value of the first field named `name` when `v` is an object, `option.none` otherwise. The `as_…` functions return the payload when `v` has that case; `as_u64` when `v` is a number that is an integer from `0` to 2⁵³, the integers that an `f64` holds exactly. `from_u64(n)` is `number(f64(n))` (§ 10.5).

Reading a record from JSON is written by hand, with `?` on each field:

```wasl
use wasl:std/json;

interface shop {
  record product { id: u32, name: string, price: u64 }

  func product_from_json(v: json::value) -> option<product> {
    return option.some(product {
      id: u32(json::as_u64(json::field(v, "id")?)?),
      name: json::as_text(json::field(v, "name")?)?,
      price: json::as_u64(json::field(v, "price")?)?,
    });
  }
}
```

## 17.4 `http`

```wasl
interface http {
  variant method {
    get, head, post, put, delete, connect, options, trace, patch,
    other(string),
  }

  record request {
    method: method,
    path: string,
    query: option<string>,
    headers: list<tuple<string, string>>,
    body: list<u8>,
  }

  record response {
    status: u16,
    headers: list<tuple<string, string>>,
    body: list<u8>,
  }

  async func handle(request: request) -> response;

  func on(request: request, m: method, pattern: string) -> bool { … }
  func segment(request: request, index: u32) -> string { … }
  func reply(status: u16, content_type: string, body: string) -> response { … }
  func not_found() -> response { … }
  func header(request: request, name: string) -> option<string> { … }
  func query_param(request: request, name: string) -> option<string> { … }
  func body_text(request: request) -> option<string> { … }
}
```

- `handle` is the function that a service writes (§ 17.1). It always returns a response: a request that the service does not handle gets a `404` or a `400` that the code returns, and a bug is a trap, which the host turns into a `500`.
- **The method** is the `method` variant of `wasi:http`, with the same cases: a method that has no case of its own is `other("PURGE")`. A misspelled case, `method::gte`, is a compile-time error (E0307). Two methods are compared with `==` (§ 8.3): for a case without payload, this compares the tags.
- **The request.** `path` is the path as received, without the query: `"/products/2"`. `query` is what follows the `?`, without it, or `option.none`. `headers` are the headers in their order, with their names in lowercase, as `wasi:http` gives them. `body` is the whole body (§ 17.1).
- `reply(status, content_type, body)` is a response with the header `content-type` and the bytes of `body`. A response with other headers is written as a record literal. `not_found()` is `reply(404, "text/plain", "not found\n")`.
- `header(request, name)` is the value of the first header named `name`, compared without case. `query_param(request, name)` is the value of the first parameter `name` of the query, with `+` and the `%XX` escapes decoded, or `option.none`. `body_text(request)` is the body if it is valid UTF-8.

**Routes.** A service chooses what to do with `if`s, tried in the order of the code: the first that matches wins.

```wasl
use wasl:std/http;
use wasl:std/json;

world catalog {
  include wasl:std/service {
    async func handle(request: request) -> response {
      if (on(request, method::get, "/products"))
        return reply(200, "application/json", json::stringify(api::all()));
      if (on(request, method::get, "/products/*"))
        return api::one(segment(request, 1));
      if (on(request, method::post, "/products"))
        return api::create(request.body);
      return not_found();
    }
  }
}
```

- `on(request, m, pattern)` is true when the method of the request is `m` and its path matches the pattern. The path and the pattern are split on `/`, without empty segments: they match when they have the same number of segments and each segment of the pattern is `*`, which stands for any one segment, or equals the segment of the path, with its `%XX` escapes decoded. `"/products/*"` matches `/products/2` and `/products/2/`, not `/products` or `/products/2/reviews`; `"/"` matches `/`.
- `segment(request, index)` is the segment of the path at `index`, counted from 0, with its `%XX` escapes decoded: for `/products/2`, `segment(request, 1)` is `"2"`. An index past the last segment is a trap, like an index past the end of a list (§ 8.13); after an `on` whose pattern has that segment, it cannot happen.
- A route calls an ordinary function of an interface of the package with its parameters, which a test calls directly (§ 3.6). `request.method == method::get` and `request.path == "/"` remain available for what `on` does not express.

## 17.5 `cli`

```wasl
interface cli {
  async func run() -> result<_, string>;

  func args() -> list<string> { … }
  func env(name: string) -> option<string> { … }
  async func print(s: string) { … }
  async func eprint(s: string) { … }
  async func read_all() -> result<string, string> { … }
}
```

- `run` is the function that a command writes (§ 17.1).
- `args()` is the list of the arguments of the command, its own name first, as WASI gives them: `weasel run wc notes.txt` gives `["wc", "notes.txt"]` (§ 14.2). It can be called from any function, async or not, as often as needed.
- `env(name)` is the value of the environment variable `name`, or `option.none`.
- `print(s)` writes `s` on the standard output, `eprint(s)` on the standard error, and both wait until the host has taken the bytes. No line feed is added.
- `read_all()` reads the standard input to its end, and returns it as text, or `result.err` if it is not valid UTF-8 or cannot be read.

`print`, `eprint` and `read_all` are async, as the streams of WASI 0.3 are: only an async function calls them (E0416), so a function that prints is an `async func`, and so are its callers. `args` and `env` are not async.

```wasl
use wasl:std/cli;
use wasl:std/text;

// wc: counts the words of the standard input, `weasel run wc < notes.txt`
world wc {
  include wasl:std/command {
    async func run() -> result<_, string> {
      if (list.len(args()) > 1)
        return result.err("usage: wc < file");
      local words = 0;
      for (line in text::lines(read_all()?)) {
        for (w in text::split(line, " ")) {
          if (w != "")
            words = words + 1;
        }
      }
      print(s32.to_string(words) + " words\n");
      return result.ok;
    }
  }
}
```

The program of `weasel init --template cli` is:

```wasl
use wasl:std/cli;

// Run it with `weasel run hello` or `weasel run hello Karim`.
world hello {
  include wasl:std/command {
    async func run() -> result<_, string> {
      local args = args();
      if (list.len(args) > 1)
        print("Hello, " + args[1] + "!\n");
      else
        print("Hello, world!\n");
      return result.ok;
    }
  }
}
```

## 17.6 `fs`

```wasl
interface fs {
  async func read_text(path: string) -> result<string, string> { … }
  async func read_bytes(path: string) -> result<list<u8>, string> { … }
  async func write_text(path: string, text: string) -> result<_, string> { … }
  async func write_bytes(path: string, bytes: list<u8>) -> result<_, string> { … }
  async func list_dir(path: string) -> result<list<string>, string> { … }
}
```

- A path is relative, with `/` separators, and is resolved in a directory that the host gives the program: with `weasel run --dir .`, `"notes.txt"` is `./notes.txt` (§ 14.2). A path that leaves that directory, through `..` or as an absolute path, is an error, as WASI requires.
- `read_text` and `read_bytes` read the whole file; `read_text` fails if it is not valid UTF-8. `write_text` and `write_bytes` create the file, or replace its contents. `list_dir` gives the names of the entries of a directory, sorted.
- An error is a message that starts with the path: `"notes.txt: no such file or directory"`.

## 17.7 Limits

What the library of 0.5 does not do, and what to do instead:

- **Streaming.** The body of a request and of a response is in memory, whole, and limited to 10 MiB for a request (§ 17.1). A service that streams, such as a large download or server-sent events, exports `wasi:http/handler` itself (§ 15.8), with streams, futures and `task.return`.
- **Outgoing requests.** `wasl:std/http` serves requests but does not send them. A service that calls another HTTP service imports `wasi:http/client` itself, until the library offers it.
- **The rest of WASI.** Clocks, random numbers, sockets and the other interfaces of WASI are imported directly (§ 2.5).
