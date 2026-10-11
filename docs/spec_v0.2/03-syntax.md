# v2.3 Syntax

This chapter amends chapter 4 (lexical structure) and replaces the grammar of chapter 5.

## v2.3.1 New tokens

**String literals.**

```ebnf
string_literal = '"' { string_char | escape } '"' ;
string_char    = (* any UTF-8 character except '"', '\', line feed and carriage return *) ;
escape         = "\" ( '"' | "'" | "\" | "n" | "r" | "t" | "u{" hex_digit { hex_digit } "}" ) ;
hex_digit      = digit | "a" | … | "f" | "A" | … | "F" ;
```

- String and char literals are the only places, with comments, where non-ASCII characters are allowed (amends § 4.1): `"Grüße"` is valid.
- `\u{…}` has one to six hexadecimal digits and designates a Unicode scalar value: at most `10FFFF`, and not a surrogate (`D800` to `DFFF`).
- A literal cannot span several lines: a line feed or carriage return before the closing `"` is an error, like a missing closing `"`, an unknown escape (`\a`) or an invalid `\u{…}` (E0006).

**Char literals.**

```ebnf
char_literal = "'" ( char_char | escape ) "'" ;
char_char    = (* any UTF-8 character except "'", '\', line feed and carriage return *) ;
```

A char literal contains exactly one Unicode scalar value, written directly or as an escape: `'a'`, `'é'`, `'\''`, `'\u{1F600}'`. An empty literal `''`, several characters `'ab'`, a missing closing `'`, or an invalid escape is an error (E0007).

**Punctuation.** `.`, `[` and `]` are new tokens (amends § 4.1 and § 4.8). `.` after the digits of a number still starts a float literal (§ 4.6), so `1.5` is a float and `inv.status` is a field access.

**Tuple indexes.** Right after a `.` token, digits are always read as an integer literal, never as a float: `t.0.1` is `t`, `.`, `0`, `.`, `1`, the element 1 of the element 0 of `t`.

**Wildcard.** A word made of a single `_` is the token `_`, used only in `result<_, E>` (§ v2.2.2). Any other identifier starting or ending with `_` remains E0004.

The characters `@`, `#`, `$` and `?` remain invalid (E0001).

## v2.3.2 Keywords and reserved words

Keywords of v0.2 (amends § 4.4):

```
world interface export global mut local func
if else while break continue return true false
record enum variant flags match case
```

Type names, also keywords:

```
bool s8 s16 s32 s64 u8 u16 u32 u64 f32 f64
char string list tuple option result
```

Reserved words:

```
import package use type resource map
own borrow stream future async await
static constructor include with as self
```

`match` and `case` were not reserved in v0.1 (§ v2.1.3).

Any keyword, type name or reserved word written where an identifier is expected is E0005: `local record: u32 = 1;`, `local while: u32 = 1;`, `func string() { }`. v0.1 reported some of these cases as E0100 (§ v2.1.3).

The case names `some`, `none`, `ok` and `err` are **not** keywords: they are always written after `option::` or `result::`, or after `case` (§ v2.4.9), so they never collide with user names.

## v2.3.3 Complete grammar of v0.2

Rules marked `(* v0.2 *)` are new or changed; the others are those of § 5.2.

```ebnf
file              = { declaration } ;
declaration       = world | interface | global ;

(* Declarations *)
world             = "world" identifier "{" { export } "}" ;
export            = "export" identifier ";" ;

interface         = "interface" identifier "{" { interface_item } "}" ;      (* v0.2 *)
interface_item    = function | record | enum | variant | flags ;              (* v0.2 *)

record            = "record" identifier "{" field { "," field } [ "," ] "}" ;           (* v0.2 *)
field             = identifier ":" [ "mut" ] type ;                                       (* v0.2 *)
enum              = "enum" identifier "{" identifier { "," identifier } [ "," ] "}" ;   (* v0.2 *)
variant           = "variant" identifier "{" case { "," case } [ "," ] "}" ;            (* v0.2 *)
case              = identifier [ "(" type ")" ] ;                                         (* v0.2 *)
flags             = "flags" identifier "{" identifier { "," identifier } [ "," ] "}" ;  (* v0.2 *)

global            = "global" identifier ":" [ "mut" ] type "=" expression ";" ;          (* v0.2 *)

function          = "func" identifier "(" [ parameters ] ")" [ "->" type ] block ;
parameters        = parameter { "," parameter } ;
parameter         = identifier ":" type ;

(* Types *)
type              = primitive_type                                                        (* v0.2 *)
                  | "char"
                  | "string"
                  | "tuple" "<" type { "," type } ">"
                  | "list" "<" [ "mut" ] type ">"
                  | "option" "<" type ">"
                  | "result" [ "<" result_args ">" ]
                  | type_name ;
result_args       = type [ "," type ] | "_" "," type ;                                    (* v0.2 *)
type_name         = identifier [ "::" identifier ] ;                                      (* v0.2 *)
primitive_type    = "bool" | "s8" | "s16" | "s32" | "s64"
                  | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" ;

(* Statements *)
block             = "{" { statement } "}" ;

statement         = local_declaration
                  | assignment
                  | if
                  | while
                  | match                                                                  (* v0.2 *)
                  | "break" ";"
                  | "continue" ";"
                  | "return" [ expression ] ";"
                  | call ";" ;

local_declaration = "local" identifier ":" type [ "=" expression ] ";" ;
assignment        = place "=" expression ";" ;                                            (* v0.2 *)
place             = identifier { selector } ;                                             (* v0.2 *)
selector          = "." ( identifier | int_literal ) | "[" expression "]" ;              (* v0.2 *)
if                = "if" "(" expression ")" block [ "else" ( if | block ) ] ;
while             = "while" "(" expression ")" block ;
match             = "match" "(" expression ")" "{" match_case { match_case }              (* v0.2 *)
                    [ "else" block ] "}" ;
match_case        = "case" identifier [ "(" identifier ")" ] block ;                      (* v0.2 *)

(* Expressions, from the lowest to the highest precedence *)
expression        = or ;
or                = and { "||" and } ;
and               = comparison { "&&" comparison } ;
comparison        = bit_or [ comparison_op bit_or ] ;
comparison_op     = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
bit_or            = bit_xor { "|" bit_xor } ;
bit_xor           = bit_and { "^" bit_and } ;
bit_and           = shift { "&" shift } ;
shift             = additive { ( "<<" | ">>" ) additive } ;
additive          = multiplicative { ( "+" | "-" ) multiplicative } ;
multiplicative    = unary { ( "*" | "/" | "%" ) unary } ;
unary             = ( "-" | "!" ) unary | postfix ;                                       (* v0.2 *)
postfix           = primary { selector } ;                                                (* v0.2 *)
primary           = int_literal
                  | float_literal
                  | string_literal                                                         (* v0.2 *)
                  | char_literal                                                           (* v0.2 *)
                  | "true" | "false"
                  | identifier
                  | path                                                                   (* v0.2 *)
                  | call
                  | conversion
                  | record_literal                                                         (* v0.2 *)
                  | flags_literal                                                          (* v0.2 *)
                  | list_literal                                                           (* v0.2 *)
                  | tuple_literal                                                          (* v0.2 *)
                  | "(" expression ")" ;

path              = path_head "::" identifier [ "::" identifier ] ;                       (* v0.2 *)
path_head         = identifier | "char" | "string" | "list" | "option" | "result" ;       (* v0.2 *)
call              = [ path_head "::" [ identifier "::" ] ] identifier "(" [ arguments ] ")" ;   (* v0.2 *)
arguments         = expression { "," expression } ;
conversion        = primitive_type "(" expression ")" ;                                   (* v0.2 *)
record_literal    = type_name "{" field_value { "," field_value } [ "," ] "}" ;           (* v0.2 *)
field_value       = identifier ":" expression ;                                           (* v0.2 *)
flags_literal     = type_name "{" [ identifier { "," identifier } [ "," ] ] "}" ;          (* v0.2 *)
list_literal      = "[" [ expression { "," expression } [ "," ] ] "]" ;                   (* v0.2 *)
tuple_literal     = "tuple" "(" expression { "," expression } [ "," ] ")" ;               (* v0.2 *)
```

## v2.3.4 Syntactic rules

In addition to § 5.3. Breaking one of them is a syntax error (E0100).

**Declarations**

- An interface contains functions and type declarations, in any order. A type can be used before its declaration.
- Records, enums, variants and flags have at least one field, case or flag: `record r { }` is invalid. Tuples have at least one element: `tuple<>` and `tuple()` are invalid.
- A world still only contains `export` items: types are not declared in worlds.

**Statements**

- The target of an assignment starts with a name, followed by any number of field accesses and indexes: `x = …`, `inv.status = …`, `rows[i].cells[j] = …`. A call cannot start an assignment target: `f().x = 1;` is invalid.
- A statement that starts with a name followed by `.` or `[` is an assignment: `inv.status;` is invalid.
- `match` is a statement, not an expression. It has at least one `case`; `else`, if present, comes last.
- Braces are required after each `case` and after `else`, as for `if`. An empty block is allowed: `case none { }`.

**Expressions**

- A `{` directly after a type name starts a record literal or a flags literal: a record literal has a `:` after its first name (`line { label: … }`), a flags literal has none (`delivery { paper }`, `delivery { }`). Because the conditions of `if`, `while` and `match` are always in parentheses (§ 5.3), `if (x) { … }` and `line { … }` are never confused.
- A path has at most two `::`: `status::paid`, `invoices::status::paid`, `invoices::discount::percent(10)`.
- Field access and indexing apply to any expression of the right type, and chain from left to right: `all[i].customer`, `sample().lines[0]`.
- Commas separate the fields of a record literal, the flags of a flags literal and the elements of a list or a tuple; a trailing comma is allowed, which keeps multi-line literals uniform.

## v2.3.5 Precedence

Level 1 of § 8.4 becomes: call, conversion, `( … )`, field access `.`, indexing `[ … ]`, all left-associative. The other levels are unchanged; `+` on strings has the precedence of `+`.
