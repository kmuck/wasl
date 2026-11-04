# 5. Grammar

## 5.1 Notation

The grammar is written in EBNF:

- `"x"` is a token written literally;
- `{ x }` is a repetition, zero or more times;
- `[ x ]` is optional;
- `a | b` is an alternative;
- each rule ends with `;`.

`identifier`, `int_literal`, `float_literal`, `string_literal` and `char_literal` are defined in chapter 4.

## 5.2 Complete grammar

```ebnf
file              = { declaration } ;
declaration       = world | interface | global ;

(* Declarations, chapter 6 *)
world             = "world" identifier "{" { export } "}" ;
export            = "export" identifier ";" ;

interface         = "interface" identifier "{" { interface_item } "}" ;
interface_item    = function | record | enum | variant | flags ;

record            = "record" identifier "{" field { "," field } [ "," ] "}" ;
field             = identifier ":" [ "mut" ] type ;
enum              = "enum" identifier "{" identifier { "," identifier } [ "," ] "}" ;
variant           = "variant" identifier "{" case { "," case } [ "," ] "}" ;
case              = identifier [ "(" type ")" ] ;
flags             = "flags" identifier "{" identifier { "," identifier } [ "," ] "}" ;

global            = "global" identifier ":" [ "mut" ] type "=" expression ";" ;

function          = "func" identifier "(" [ parameters ] ")" [ "->" type ] block ;
parameters        = parameter { "," parameter } ;
parameter         = identifier ":" type ;

(* Types, chapter 7 *)
type              = primitive_type
                  | "char"
                  | "string"
                  | "tuple" "<" type { "," type } ">"
                  | "list" "<" [ "mut" ] type ">"
                  | "option" "<" type ">"
                  | "result" [ "<" result_args ">" ]
                  | type_name ;
result_args       = type [ "," type ] | "_" "," type ;
type_name         = identifier [ "::" identifier ] ;
primitive_type    = "bool" | "s8" | "s16" | "s32" | "s64"
                  | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" ;

(* Statements *)
block             = "{" { statement } "}" ;

statement         = local_declaration
                  | assignment
                  | if
                  | while
                  | match
                  | "break" ";"
                  | "continue" ";"
                  | "return" [ expression ] ";"
                  | call ";"
                  | "_" "=" call ";" ;

local_declaration = "local" identifier ":" type [ "=" expression ] ";" ;
assignment        = place "=" expression ";" ;
place             = identifier { selector } ;
selector          = "." ( identifier | int_literal ) | "[" expression "]" ;
if                = "if" "(" expression ")" block [ "else" ( if | block ) ] ;
while             = "while" "(" expression ")" block ;
match             = "match" "(" expression ")" "{" match_case { match_case }
                    [ "else" block ] "}" ;
match_case        = "case" identifier [ "(" identifier ")" ] block ;

(* Expressions, from the lowest to the highest precedence, § 8.4 *)
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
unary             = ( "-" | "!" ) unary | postfix ;
postfix           = primary { selector } ;
primary           = int_literal
                  | float_literal
                  | string_literal
                  | char_literal
                  | "true" | "false"
                  | identifier
                  | path
                  | call
                  | conversion
                  | record_literal
                  | flags_literal
                  | list_literal
                  | tuple_literal
                  | "(" expression ")" ;

path              = path_head "::" identifier [ "::" identifier ] ;
path_head         = identifier | "char" | "string" | "list" | "option" | "result"
                  | int_type ;
int_type          = "s8" | "s16" | "s32" | "s64" | "u8" | "u16" | "u32" | "u64" ;
call              = [ path_head "::" [ identifier "::" ] ] identifier "(" [ arguments ] ")" ;
arguments         = expression { "," expression } ;
conversion        = primitive_type "(" expression ")" ;
record_literal    = type_name "{" field_value { "," field_value } [ "," ] "}" ;
field_value       = identifier ":" expression ;
flags_literal     = type_name "{" [ identifier { "," identifier } [ "," ] ] "}" ;
list_literal      = "[" [ expression { "," expression } [ "," ] ] "]" ;
tuple_literal     = "tuple" "(" expression { "," expression } [ "," ] ")" ;
```

## 5.3 Syntactic rules

These rules follow from the grammar; they are listed because they differ from many other languages. Breaking one of them is a syntax error (E0100).

**Blocks and conditions**

- Braces are **required** after `if`, `else`, `while`, each `case` and the `else` of a `match`: `if (x > 0) return x;` is invalid. An empty block is allowed: `case none { }`.
- The condition of `if` and `while`, and the expression of `match`, are **always in parentheses**: `if x > 0 { … }` is invalid.
- `else if` chains a new `if` directly; it is the only form where `else` is not followed by `{`.
- A block is not a statement: a `{ … }` cannot appear on its own inside a function body.

**Statements**

- Every simple statement ends with `;`. No `;` follows a `}`.
- Only an assignment or a function call can start with a name. A statement that starts with a name followed by anything other than `=`, `(`, `::`, `.` or `[` is invalid: `x + 1;` and `x;` are invalid.
- The target of an assignment starts with a name, followed by any number of field accesses and indexes: `x = …`, `c.hits = …`, `rows[i].cells[j] = …`. A call cannot start an assignment target: `f().x = 1;` is invalid.
- A statement that starts with a name followed by `.` or `[` is an assignment: `inv.status;` is invalid.
- An expression is not a statement: `1;` and `f(1) + 2;` are invalid. A function call is the only expression that can be used as a statement (§ 9.6).
- `_ = f(…);` discards the value of a call (§ 9.6). Only a call can follow `_ =`: `_ = x;` is invalid.
- `match` is a statement, not an expression. It has at least one `case`; `else`, if present, comes last.
- There are no compound assignments (`+=`, `-=`, …) and no `++` / `--`: write `i = i + 1;`.
- `local` always has a type: `local x = 5;` is invalid.

**Declarations**

- A `global` is declared at the top level, never inside an interface.
- An interface contains functions and type declarations, in any order. A type can be used before its declaration.
- A `world` only contains `export` items: types are not declared in worlds.
- Records, enums, variants and flags have at least one field, case or flag: `record r { }` is invalid. Tuples have at least one element: `tuple<>` and `tuple()` are invalid.

**Expressions**

- A comparison is not associative: `a < b < c` is invalid; write `a < b && b < c`.
- A conversion is the name of a primitive type followed by a parenthesized expression: `u64(n)`. A type name is not an expression on its own.
- An integer type followed by `::` starts a call to one of its built-in functions, `u32::wrapping_add(a, b)`; followed by `(`, it is a conversion, `u32(x)` (§ 8.17).
- A path has at most two `::`: `status::paid`, `invoices::status::paid`, `invoices::discount::percent(10)`. A call has an optional path: `fib(n)`, `math::fib(n)`, `string::len(s)`.
- A `{` directly after a type name starts a record literal or a flags literal: a record literal has a `:` after its first name (`line { label: … }`), a flags literal has none (`delivery { paper }`, `delivery { }`). Because the conditions of `if`, `while` and `match` are always in parentheses, `if (x) { … }` and `line { … }` are never confused.
- Field access and indexing apply to any expression of the right type, and chain from left to right: `all[i].customer`, `sample().lines[0]`.
- Commas separate the fields of a record literal, the flags of a flags literal and the elements of a list or a tuple; a trailing comma is allowed, which keeps multi-line literals uniform.
