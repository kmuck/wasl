# WaSL v0.5 Specification

Status: **draft, in progress, provisional until v0.6**. This document continues the frozen [v0.4 specification](../spec_v0.4/README.md), with the same section numbers, and receives the features of version 0.5. At this point it describes exactly the language that `waslc` 0.4 implements: the language of v0.3, and the features of 0.4: async functions, streams and futures, `task::return`, tuple destructuring, and the commands `weasel run` and `weasel serve`. The target programs of 0.4 are `examples/hello` for WASI 0.3 (§ 15.7) and `examples/hello-http` (§ 15.8). v0.3 merged the [v0.1](../spec_v0.1/README.md) and [v0.2](../spec_v0.2/README.md) specifications into one complete specification, without changing any of their rules, and added imports, dependencies, resources, error propagation and implementations of foreign interfaces. The target programs of 0.3 are `examples/catalog` with `examples/checkout` (§ 15.5), and `examples/hello` (§ 15.6).

This specification defines WaSL completely: the language, how it maps onto WebAssembly, WasmGC and the Component Model, the compiler contract and the build tool. It is written so that a reader who discovers the project, human or AI agent, can understand everything the language is without any other context, and can implement, test or extend it. The earlier versions are frozen and are no longer needed to read it.

## What WaSL is

WaSL is a programming language that sits **one-to-one on top of WebAssembly, WasmGC, the Component Model and WASI**. Its types are the types of WIT, its structure is the structure of WIT (`interface`s and `world`s), its data lives on the WasmGC heap, and each world is compiled into a WebAssembly component that runs on wasmtime.

```wasl
world billing {
  export invoices;
}

interface invoices {

  record line {
    label: string,
    quantity: u32,
    unit_price: u64,
  }

  enum invoice_error {
    no_lines,
    zero_quantity,
  }

  func total(lines: list<line>) -> result<u64, invoice_error> {
    if (list::len(lines) == 0) {
      return result::err(invoice_error::no_lines);
    }
    local sum: u64;
    local i: u32;
    while (i < list::len(lines)) {
      if (lines[i].quantity == 0) {
        return result::err(invoice_error::zero_quantity);
      }
      sum = sum + u64(lines[i].quantity) * lines[i].unit_price;
      i = i + 1;
    }
    return result::ok(sum);
  }

}
```

```sh
weasel build billing                                                                    # → build/billing.wasm
weasel call billing invoices::total '[{label: "pen", quantity: 3, unit-price: 150}]'    # → ok(450)
weasel call billing invoices::total '[]'                                                # → err(no-lines)
```

## Chapters

The number of a chapter is the number of its sections: § 8.2 is section 2 of chapter 8, in `08-typing.md`. Source code comments refer to sections with this notation.

| § | Chapter | Content |
|---|---|---|
| 1 | [Overview](01-overview.md) | Goals, design principles and decisions, scope, glossary, changes from v0.1, v0.2 and v0.3 |
| 2 | [Component Model](02-component-model.md) | How WaSL maps onto packages, interfaces, worlds and components; the two kinds of exports; imports; entry points; resources; async functions and tasks |
| 3 | [Projects](03-projects.md) | Project layout, manifest, source files, outputs, dependencies |
| 4 | [Lexical structure](04-lexical.md) | Characters, comments, identifiers, keywords, literals, punctuation |
| 5 | [Grammar](05-grammar.md) | Complete EBNF grammar and syntactic rules |
| 6 | [Declarations](06-declarations.md) | Package namespace, `world` with `import` and `export`, `interface`, `global`, `func` |
| 7 | [Types](07-types.md) | Primitive types, built-in and declared types, recursion, mutability, types in the WIT, resources, streams and futures |
| 8 | [Typing](08-typing.md) | Literals, operators, conversions, constructors, field access, lists, `match`, built-in functions, globals, `?`, resource calls, async calls, `task::return` |
| 9 | [Names and scopes](09-names.md) | Name resolution, paths, locals and their default values, tuple destructuring, assignment, calls as statements, loops, handles |
| 10 | [Runtime semantics](10-semantics.md) | Evaluation order, integer and float arithmetic, conversions, references and sharing, traps, resources and imports, tasks |
| 11 | [Diagnostics](11-diagnostics.md) | Error format, order of analysis, error codes |
| 12 | [Mapping to Wasm and WasmGC](12-wasm-mapping.md) | Core module generated for a world, representation of each type, resources and handles, `?`, async and streams |
| 13 | [Mapping to the Component Model](13-component-mapping.md) | Generated WIT, names, core exports, Canonical ABI, copies through linear memory, imports, resources at the boundary, foreign exports, async at the boundary |
| 14 | [Tooling](14-tooling.md) | The `weasel` commands, including `run` and `serve`, values in WAVE, the `waslc` library API |
| 15 | [Examples](15-examples.md) | The target programs, invalid programs |
| 16 | [Open questions](16-open-questions.md) | What this specification leaves undecided |

The direction of the language from one version to the next is in [ROADMAP.md](../../ROADMAP.md), which replaces chapter 16 of v0.1.

## How to read this specification

- **To understand the language**: chapters 1 and 2, then the examples of chapter 15.
- **To write WaSL code**: chapters 3 to 10.
- **To work on the compiler (`waslc`)**: everything, with chapters 11 to 13 for the output.
- **To work on the build tool (`weasel`)**: chapters 3, 11 and 14.

## Conventions

- **must** / **must not**: a requirement. A program, compiler or tool that does not follow it does not conform.
- **invalid**: a program that a conforming compiler rejects with an error (chapter 11).
- Any construct that this specification does not describe is invalid.
- Grammar rules use EBNF (§ 5.1). Code is shown in `monospace`; `T`, `S` stand for types, `e` for an expression.
- A **scalar type** is a primitive type, `char`, an enum or a flags type; a **heap type** is any other type (§ 7.2).
- A feature marked **specified, not available yet** belongs to the language model described here, but the current compiler rejects it. It is described so that the model is complete and so that later versions extend it without changing it.

## Correspondence with v0.1, v0.2, v0.3 and v0.4

Sections of v0.3 and v0.4 keep their number in v0.5. Sections of v0.1 keep their number, except in chapter 1. Each section of v0.2 has moved into the chapter of the same subject:

| v0.2 | v0.5 | | v0.2 | v0.5 |
|---|---|---|---|---|
| § v2.1.1 | § 1.4 | | § v2.5.1 | § 10.8 |
| § v2.1.2 | § 1.3 | | § v2.5.2 | § 10.1 |
| § v2.1.3 | § 1.7 | | § v2.5.3 | § 10.9 |
| § v2.2.1 | § 7.2, § 7.5 | | § v2.5.4 | § 10.10 |
| § v2.2.2 | § 7.6 | | § v2.5.5 | § 10.7 |
| § v2.2.3 | § 7.7 | | § v2.5.6 | § 10.2, § 10.5 |
| § v2.2.4 | § 7.8 | | § v2.6.1 | § 12.5 |
| § v2.2.5 | § 7.9 | | § v2.6.2 | § 12.6 |
| § v2.2.6 | § 7.10 | | § v2.6.3 | § 12.7 |
| § v2.2.7 | § 7.11 | | § v2.6.4 | § 12.8 |
| § v2.3.1 | § 4.1, § 4.8 to § 4.10 | | § v2.6.5 | § 12.9 |
| § v2.3.2 | § 4.4 | | § v2.6.6 | § 12.1 |
| § v2.3.3 | § 5.2 | | § v2.7.1 | § 13.1 |
| § v2.3.4 | § 5.3 | | § v2.7.2 | § 13.5 |
| § v2.3.5 | § 8.4 | | § v2.7.3 | § 13.6 |
| § v2.4.1 | § 8.2 | | § v2.7.4 | § 13.7 |
| § v2.4.2 | § 9.8 | | § v2.7.5 | § 13.8 |
| § v2.4.3 | § 8.10 | | § v2.7.6 | § 13.2 |
| § v2.4.4 | § 8.11 | | § v2.7.7 | § 13.9 |
| § v2.4.5 | § 8.12 | | § v2.8.1 | § 11.2 |
| § v2.4.6 | § 8.13 | | § v2.8.2 | § 11.3 |
| § v2.4.7 | § 8.14 | | § v2.8.3 | § 11.6 |
| § v2.4.8 | § 8.15 | | § v2.9.1 | § 14.5 |
| § v2.4.9 | § 8.16 | | § v2.9.2 | § 14.6 |
| § v2.4.10 | § 8.3 | | § v2.10.1 | § 15.3 |
| § v2.4.11 | § 8.17, § 8.5 | | § v2.10.2 | § 15.4 |
| § v2.4.12 | § 8.18 | | § v2.11 | § 16 |
| § v2.4.13 | § 9.4 | | | |
| § v2.4.14 | § 9.5 | | | |
| § v2.4.15 | § 8.8 | | | |
| § v2.4.16 | § 9.6 | | | |

In chapter 1, v0.1's § 1.3 (scope) became § 1.4, § 1.4 (pipeline) § 1.5 and § 1.5 (glossary) § 1.6. v0.1's § 13.5 (building the component) became § 13.9, and § 15.3 (invalid programs) § 15.4.
