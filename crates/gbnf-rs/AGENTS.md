# gbnf-rs
Generate GBNF grammar ASTs from `schemars::Schema` inputs.

## Dependencies
- schemars
  - parse and inspect JSON Schemas
- insta
  - snapshot testing of generated grammars

## Features
- Builds an AST of rules that can be rendered to GBNF
- Supports referencing external non-terminal symbols like `ws`, `string`, and `number`
- Ensures internal rule names are unique across generations
- Top-level rule names are sanitized to letters, digits, and `-`, prefixed with `r` when needed, and suffixed with a unique counter
- Avoids emitting redundant rules for simple property types by inlining their expressions
- Understands schema `type` arrays, ignoring `null` when present
- Handles array schemas by expanding item expressions into comma-separated sequences
- Snapshot tests cover schemas with required and optional fields, including nested structs and arrays of structs
- Resolves `$ref` definitions, reusing generated rules for shared schemas

## Constraints
- Generated object rules include all properties
  - required properties appear first
  - optional properties follow in fixed, deterministic order
    - each optional property appears at most once (`?` semantics)
    - when requireds exist, each optional is prefixed by a comma if present
    - when no requireds exist, the first present optional has no leading comma
    - when no requireds exist, optional properties can start at any position and may skip middle properties while preserving order and uniqueness
      - implemented via recursive choices over downstream chains (O(n^2) size)
