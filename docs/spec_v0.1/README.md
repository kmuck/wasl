# WaSL v0.1 Specification

Status: **draft, implemented**. Version 0.1 of the language is implemented by [`waslc`](../../waslc) (compiler library) and [`weasel`](../../weasel) (build tool).

This specification defines WaSL v0.1 completely: the language, how it maps onto WebAssembly and the Component Model, the compiler contract and the build tool. It is written so that a reader who discovers the project, human or AI agent, can understand everything that v0.1 must be without any other context, and can implement, test or extend it.

## What v0.1 is

WaSL is a programming language that sits **one-to-one on top of WebAssembly, the Component Model and WasmGC**. Version 0.1 is the minimal core of that idea: numbers, booleans, functions, globals and control flow, organized in `interface`s and `world`s exactly as in WIT, and compiled into WebAssembly components that run on wasmtime.

```wasl
world calculator {
  export math;
}

global calls: mut u32 = 0;

interface math {

  func fib(n: u32) -> u64 {
    calls = calls + 1;
    local a: u64 = 0;
    local b: u64 = 1;
    local i: u32;
    while (i < n) {
      local next: u64 = a + b;
      a = b;
      b = next;
      i = i + 1;
    }
    return a;
  }

}
```

```sh
weasel build calculator      # → build/calculator.wasm
weasel call math::fib 10     # → 55
```

## Chapters

The number of a chapter is the number of its sections: § 8.2 is section 2 of chapter 8, in `08-typing.md`. Source code comments refer to sections with this notation.

| § | Chapter | Content |
|---|---|---|
| 1 | [Overview](01-overview.md) | Goals, design principles, scope of v0.1, glossary |
| 2 | [Component Model](02-component-model.md) | How WaSL maps onto packages, interfaces, worlds and components; the two kinds of exports; entry points |
| 3 | [Projects](03-projects.md) | Project layout, manifest, source files, outputs |
| 4 | [Lexical structure](04-lexical.md) | Characters, comments, identifiers, keywords, literals, punctuation |
| 5 | [Grammar](05-grammar.md) | Complete EBNF grammar and syntactic rules |
| 6 | [Declarations](06-declarations.md) | Package namespace, `world`, `interface`, `global`, `func` |
| 7 | [Types](07-types.md) | Primitive types and their categories |
| 8 | [Typing](08-typing.md) | Typing rules: literals, operators, precedence, conversions, returns |
| 9 | [Names and scopes](09-names.md) | Name resolution, scopes, assignment, loops |
| 10 | [Runtime semantics](10-semantics.md) | Evaluation order, integer and float arithmetic, conversions, traps |
| 11 | [Diagnostics](11-diagnostics.md) | Error format, error codes, order of analysis |
| 12 | [Mapping to Wasm](12-wasm-mapping.md) | Core module generated for a world |
| 13 | [Mapping to the Component Model](13-component-mapping.md) | Generated WIT, names, core exports, Canonical ABI |
| 14 | [Tooling](14-tooling.md) | The `weasel` commands and the `waslc` library API |
| 15 | [Examples](15-examples.md) | A complete valid project, invalid programs |
| 16 | [Beyond v0.1](16-roadmap.md) | What comes next, open questions |

## How to read this specification

- **To understand the language**: chapters 1 and 2, then the example of chapter 15.
- **To write WaSL code**: chapters 3 to 10.
- **To work on the compiler (`waslc`)**: everything, with chapters 11 to 13 for the output.
- **To work on the build tool (`weasel`)**: chapters 3, 11 and 14.

## Conventions

- **must** / **must not**: a requirement. A program, compiler or tool that does not follow it does not conform to v0.1.
- **invalid**: a program that a conforming compiler rejects with an error (chapter 11).
- Any construct that this specification does not describe is invalid in v0.1.
- Grammar rules use EBNF (§ 5.1). Code is shown in `monospace`; `T`, `S` stand for types, `e` for an expression.
- A feature marked **specified, not available in v0.1** belongs to the language model described here, but a v0.1 compiler rejects it. It is described so that the model is complete and so that later versions extend it without changing it.
