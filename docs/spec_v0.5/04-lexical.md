# 4. Lexical structure

The lexer splits each source file into **tokens**: keywords, identifiers, literals and punctuation. Whitespace and comments separate tokens and are otherwise ignored.

## 4.1 Encoding and characters

- Source files are encoded in **UTF-8**.
- Outside comments, string literals and char literals, only ASCII characters are allowed. Any other character is an error (E0001), e.g. `é` in an identifier; `"Grüße"` is valid.
- Whitespace characters are: space (U+0020), tab (U+0009), line feed (U+000A) and carriage return (U+000D).
- Any ASCII character that is not whitespace and does not start a token is an error (E0001). In particular, `@`, `#` and `$` are invalid.

## 4.2 Comments

- `//` starts a comment that extends to the end of the line. A comment may contain any UTF-8 character.
- `///` starts a **doc comment**, which also extends to the end of the line. A run of doc comments documents the declaration that follows it: a world, an interface, a function, a type, a field, a case, a flag or a resource item. The compiler copies it into the generated WIT, which has exactly the same comments (§ 13.1), so that the tools that read WIT (editors, bindings generators, registries) show it. A doc comment that is followed by anything else, such as a statement or another comment, is an ordinary comment: it is ignored. `////` and more are ordinary comments.
- There is no other form of comment (no `/* … */`).

## 4.3 Identifiers

```ebnf
identifier = word { "_" word } ;
word       = letter { letter | digit } ;
letter     = "a" | … | "z" ;
digit      = "0" | … | "9" ;
```

An identifier is made of lowercase words separated by single underscores (`snake_case`):

| Valid | Invalid | Reason | Code |
|---|---|---|---|
| `fib` | `Fib` | uppercase letter | E0004 |
| `fib_rec` | `_x` | leading `_` | E0004 |
| `circle_area` | `x_` | trailing `_` | E0004 |
| `x2` | `a__b` | doubled `_` | E0004 |
| `max_u8` | `a_1` | a word starts with a digit | E0004 |
| | `élan` | non-ASCII character | E0001 |

These rules guarantee that every identifier converts to a valid WIT identifier by replacing `_` with `-` (§ 13.2). An identifier cannot be a keyword or a reserved word (§ 4.4).

The lexer reads the longest run of ASCII letters, digits and `_` starting with a letter or `_`, then checks it against these rules, then against the keywords. A run made of a single `_` is the wildcard token (§ 4.10).

## 4.4 Keywords and reserved words

**Keywords:**

```
world interface export import global mut local func
if else while for in break continue return true false
record enum variant flags match case type use as
resource constructor static self borrow
async task include
```

**Type names**, also keywords:

```
bool s8 s16 s32 s64 u8 u16 u32 u64 f32 f64
char string list tuple option result
stream future stream_writer future_writer
```

**Reserved words**, which are not used yet but cannot be identifiers, for future versions and for compatibility with WIT:

```
package map own
await subtask
with
```

Any keyword, type name or reserved word written where an identifier is expected is an error (E0005): `local record: u32 = 1;`, `local while: u32 = 1;`, `func string() { }`.

`async`, `task`, `stream`, `future`, `stream_writer` and `future_writer` are used from 0.4 (§ 2.8, § 7.13, § 8.21). They are all keywords. `task` names the built-in functions of the current task, as `list` names those of lists: `task.return(v)`. `subtask` is reserved for concurrency inside a task (§ 16).

`for` and `in` (§ 9.10), `type` (§ 7.14), `use` and `as` (§ 6.6), `include` (§ 2.4.3) are used from 0.5. `in` was not reserved before: an identifier named `in` is E0005 from 0.5.

**`await`.** WaSL has no `await`: a call to an async function waits by itself (§ 2.8). `await` written anywhere is an error of its own (E0101), whose message says to remove it, so that code written by habit from Rust or JavaScript is corrected at once: `client::send(req).await?` and `await client::send(req)` both report E0101.

The case names `some`, `none`, `ok` and `err` are **not** keywords: they are always written after `option.` or `result.`, or after `case` (§ 8.16), so they never collide with user names.

## 4.5 Integer literals

```ebnf
int_literal   = "0" | nonzero_digit { digit } ;
nonzero_digit = "1" | … | "9" ;
```

- Only decimal digits: no separator (`1_000`), no prefix (`0x`, `0b`, `0o`), no suffix (`10u8`).
- A leading zero is forbidden, except for `0` itself: `007` is an error (E0002).
- An integer literal immediately followed by a letter or a `_` is malformed: `0x1F`, `1e10`, `123abc` and `1_000` are errors (E0002). A `.` after the digits starts a float literal (§ 4.6).
- An integer literal has no sign: `-5` is the unary operator `-` applied to the literal `5`. Its range is checked when it is typed (§ 8.2); a literal too large for any type is reported then (E0401).

## 4.6 Float literals

```ebnf
float_literal = digits "." digits ;
digits        = digit { digit } ;
```

- At least one digit on each side of the point: `1.0`, `0.5`, `3.14159`, `007.5`.
- No exponent and no special value: `1.` (E0003), `.5` (E0003), `1.5e3` (E0003) and `1.2.3` (E0003) are errors. `1e10` is a malformed integer literal (E0002, § 4.5).
- There is no literal for infinity or NaN; they can only result from computations (§ 10.3).
- **Tuple indexes.** Right after a `.` token, digits are always read as an integer literal, never as a float: `t.0.1` is `t`, `.`, `0`, `.`, `1`, the element 1 of the element 0 of `t` (§ 8.12).

## 4.7 Boolean literals

`true` and `false`.

## 4.8 String literals

```ebnf
string_literal = '"' { string_char | escape } '"' ;
string_char    = (* any UTF-8 character except '"', '\', line feed and carriage return *) ;
escape         = "\" ( '"' | "'" | "\" | "n" | "r" | "t" | "u{" hex_digit { hex_digit } "}" ) ;
hex_digit      = digit | "a" | … | "f" | "A" | … | "F" ;
```

- `\u{…}` has one to six hexadecimal digits and designates a Unicode scalar value: at most `10FFFF`, and not a surrogate (`D800` to `DFFF`).
- A literal cannot span several lines: a line feed or carriage return before the closing `"` is an error, like a missing closing `"`, an unknown escape (`\a`) or an invalid `\u{…}` (E0006).

## 4.9 Char literals

```ebnf
char_literal = "'" ( char_char | escape ) "'" ;
char_char    = (* any UTF-8 character except "'", '\', line feed and carriage return *) ;
```

A char literal contains exactly one Unicode scalar value, written directly or as an escape: `'a'`, `'é'`, `'\''`, `'\u{1F600}'`. An empty literal `''`, several characters `'ab'`, a missing closing `'`, or an invalid escape is an error (E0007).

## 4.10 Punctuation and operators

```
{  }  (  )  [  ]  ,  ;  :  ::  ->  =  .  _
+  -  *  /  %  &  |  ^  <<  >>
== != <  <= >  >=  && ||  !  ?
```

- The lexer always takes the longest possible token: `<<` is one token, not two `<`; `::` is one token, not two `:`; `->` is one token, not `-` followed by `>`.
- `.` after the digits of a number starts a float literal (§ 4.6), so `1.5` is a float and `inv.status` is a field access.
- `?` is the error propagation operator, written after an expression (§ 8.19). It does not combine with `.` or `:`: there is no `?.` or `??`.
- **Wildcard.** A word made of a single `_` is the token `_`, used only in `result<_, E>` (§ 7.6) and in a destructuring, `local (a: A, _) = e;` (§ 9.4). Any other identifier starting or ending with `_` remains E0004.
