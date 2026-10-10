# 4. Lexical structure

The lexer splits each source file into **tokens**: keywords, identifiers, literals and punctuation. Whitespace and comments separate tokens and are otherwise ignored.

## 4.1 Encoding and characters

- Source files are encoded in **UTF-8**.
- Outside comments, only ASCII characters are allowed. Any other character is an error (E0001), e.g. `é` in an identifier.
- Whitespace characters are: space (U+0020), tab (U+0009), line feed (U+000A) and carriage return (U+000D).
- Any ASCII character that is not whitespace and does not start a token (for instance `@`, `#`, `$`, `?`, `.`, `[`, `"`) is an error (E0001).

## 4.2 Comments

- `//` starts a comment that extends to the end of the line. A comment may contain any UTF-8 character.
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

The lexer reads the longest run of ASCII letters, digits and `_` starting with a letter or `_`, then checks it against these rules, then against the keywords.

## 4.4 Keywords and reserved words

**Keywords** used in v0.1:

```
world interface export global mut local func
if else while break continue return true false
```

**Type names**, also keywords:

```
bool s8 s16 s32 s64 u8 u16 u32 u64 f32 f64
```

**Reserved words**, which are not used in v0.1 but cannot be identifiers, for future versions and for compatibility with WIT:

```
import package use type record enum variant flags resource
list option result tuple string char map
own borrow stream future async await
static constructor include with as self
```

Using a reserved word as an identifier is an error (E0005), e.g. `local record: u32 = 1;`.

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

## 4.7 Boolean literals

`true` and `false`.

## 4.8 Punctuation and operators

```
{  }  (  )  ,  ;  :  ::  ->  =
+  -  *  /  %  &  |  ^  <<  >>
== != <  <= >  >=  && ||  !
```

The lexer always takes the longest possible token: `<<` is one token, not two `<`; `::` is one token, not two `:`; `->` is one token, not `-` followed by `>`.
