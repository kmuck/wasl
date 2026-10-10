# 5. Grammar

## 5.1 Notation

The grammar is written in EBNF:

- `"x"` is a token written literally;
- `{ x }` is a repetition, zero or more times;
- `[ x ]` is optional;
- `a | b` is an alternative;
- each rule ends with `;`.

`identifier`, `int_literal` and `float_literal` are defined in chapter 4.

## 5.2 Complete grammar of v0.1

```ebnf
file              = { declaration } ;
declaration       = world | interface | global ;

(* Declarations, chapter 6 *)
world             = "world" identifier "{" { export } "}" ;
export            = "export" identifier ";" ;

interface         = "interface" identifier "{" { function } "}" ;

global            = "global" identifier ":" [ "mut" ] type "=" global_init ";" ;
global_init       = [ "-" ] int_literal | [ "-" ] float_literal | "true" | "false" ;

function          = "func" identifier "(" [ parameters ] ")" [ "->" type ] block ;
parameters        = parameter { "," parameter } ;
parameter         = identifier ":" type ;

type              = "bool" | "s8" | "s16" | "s32" | "s64"
                  | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" ;

(* Statements *)
block             = "{" { statement } "}" ;

statement         = local_declaration
                  | assignment
                  | if
                  | while
                  | "break" ";"
                  | "continue" ";"
                  | "return" [ expression ] ";"
                  | call ";" ;

local_declaration = "local" identifier ":" type [ "=" expression ] ";" ;
assignment        = identifier "=" expression ";" ;
if                = "if" "(" expression ")" block [ "else" ( if | block ) ] ;
while             = "while" "(" expression ")" block ;

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
unary             = ( "-" | "!" ) unary | primary ;
primary           = int_literal
                  | float_literal
                  | "true" | "false"
                  | identifier
                  | call
                  | conversion
                  | "(" expression ")" ;

call              = [ identifier "::" ] identifier "(" [ arguments ] ")" ;
arguments         = expression { "," expression } ;
conversion        = type "(" expression ")" ;
```

## 5.3 Syntactic rules

These rules follow from the grammar; they are listed because they differ from many other languages. Breaking one of them is a syntax error (E0100).

**Blocks and conditions**

- Braces are **required** after `if`, `else` and `while`: `if (x > 0) return x;` is invalid.
- The condition of `if` and `while` is **always in parentheses**: `if x > 0 { … }` is invalid.
- `else if` chains a new `if` directly; it is the only form where `else` is not followed by `{`.
- A block is not a statement: a `{ … }` cannot appear on its own inside a function body.

**Statements**

- Every simple statement ends with `;`. No `;` follows a `}`.
- Only an assignment or a function call can start with a name. A statement that starts with a name followed by anything other than `=`, `(` or `::` is invalid: `x + 1;` and `x;` are invalid.
- An expression is not a statement: `1;` and `f(1) + 2;` are invalid. A function call is the only expression that can be used as a statement (§ 9.6).
- There are no compound assignments (`+=`, `-=`, …) and no `++` / `--`: write `i = i + 1;`.
- `local` always has a type: `local x = 5;` is invalid.

**Declarations**

- A `global` is declared at the top level, never inside an interface.
- The initial value of a `global` is a literal, optionally preceded by `-`, and nothing else: `global g: u32 = 1 + 1;` is invalid.
- An `interface` only contains `func` declarations; a `world` only contains `export` items.

**Expressions**

- A comparison is not associative: `a < b < c` is invalid; write `a < b && b < c`.
- A conversion is the name of a type followed by a parenthesized expression: `u64(n)`. A type name is not an expression on its own.
- A call has an optional interface path: `fib(n)` or `math::fib(n)`. There is only one level of path.
