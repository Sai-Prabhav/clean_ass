# clean_ass — Design & Working Agreement

`clean_ass` is a formatter for NASM/YASM assembly files (Intel syntax). This document
records the formatting contract and why each choice was made, so future changes don't
silently break it.

---

## Working agreement (contributors and AI agents)

- When an action requires permission — editing files, running a command, network access,
  installing tools, changing configuration — **ask first and wait**.
- Do **not** look for an alternative route, workaround, or clever bypass around a missing
  permission. Stop and ask.
- Do not treat silence, a vague answer, or partial approval as consent for the rest.
- Never overwrite or delete work you did not create without asking.

---

## Scope

**In scope**

- NASM and YASM, Intel syntax (the two share the same source syntax; `section` and
  `segment` are both accepted).
- 16/32/64-bit x86 source files, including preprocessor directives and multi-line macros.

**Out of scope**

- AT&T/GAS syntax, MASM/TASM syntax, Go assembler syntax.
- Semantic changes: the formatter never rewrites instructions, reorders lines, resolves
  macros/`equ`s, evaluates expressions, or touches string data.
- Being a compiler or linter. A line it cannot understand is passed through with only
  trailing whitespace removed.

---

## Decision log

| # | Decision | Why | Rejected alternative |
|---|---|---|---|
| 1 | Line-oriented lexer, not a full parser | NASM's preprocessor and `%macro` system is effectively a programming language; a full AST is a maintenance trap and risks corrupting valid source | Full parser |
| 2 | One operand column for the entire file | Explicit user requirement: every first operand starts at the same column, everywhere | Per-block alignment |
| 3 | The longest mnemonic always sets the column; no exemptions | A hard column guarantee was chosen over compactness | Percentile / outlier exemption |
| 4 | Adaptive gap: generous when mnemonics are short, tight when long | 8 spaces of padding is pleasant on integer code but wasteful next to `vpbroadcastmb2q` | Fixed gap |
| 5 | Indirect operands (`arg2`, `arg3`, ...) can be column-aligned, opt-in | Regular data tables (`db 1, 2, 3`) benefit; mixed code gets very wide | Always align / never align |
| 6 | Comments align to `longest code line + gap` | Same "longest sets the column" rule as instructions, instead of an arbitrary column 40 | Fixed column |
| 7 | Directives are indented and aligned like instructions | One visual field for every statement; `section`/`global` participate in the column width | Flush-left directives |
| 8 | Mnemonics and directive keywords lowercased; symbols/strings untouched | Case noise is a large part of ugly asm diffs, but renaming symbols would be destructive | Uppercase / preserve |
| 9 | Unknown input passes through, trailing whitespace trimmed | A formatter that mangles code is worse than no formatter | Error out |
| 10 | Byte-oriented I/O with ASCII classification | Survives non-UTF-8 comments and arbitrary bytes in strings | UTF-8 strings |
| 11 | stdout by default; `-i` writes; `--check` for CI | Composable with editors and pipes; safe by default | In-place by default |
| 12 | Zero dependencies, hand-rolled CLI and TOML subset | Build stays trivial and auditable; no supply-chain surface | clap + serde/toml |
| 13 | Alignment is computed from tokens, never from existing whitespace | Guarantees idempotency: `format(format(x)) == format(x)` | Preserve existing padding |

---

## Formatting model

### Fields

Every code line is a row of fields:

```
[indent][mnemonic][gap][arg1][gap][arg2] ... [gap][; comment]
```

- `indent` = 4 spaces by default.
- `mnemonic` includes instruction prefixes (`lock`, `rep`, `repne`, `xacquire`,
  segment overrides such as `cs`) joined by a single space, and counts toward the
  column width as a unit. `rep movsb` is one field, not `rep` with operand `movsb`.
- `equ` lines are treated as `[label] equ [value]`.
- `times` is the mnemonic; the remainder is one opaque operand, so
  `times 510-($-$$) db 0` keeps its internal spacing.

### Adaptive gap

```
M        = max mnemonic length over every parseable code line in the file
gap(M)   = clamp(align_gap - max(0, M - soft_limit), min_gap, align_gap)
arg1_col = indent + M + gap(M)
```

Defaults: `indent = 4`, `align_gap = 8`, `min_gap = 2`, `soft_limit = 8`.

Preprocessor/comment/raw/label-only lines are excluded from `M`. `equ` lines do not
count toward the mnemonic measurement.

| longest mnemonic | `M` | `gap` (align_gap=8) | `arg1_col` | `gap` (align_gap=4) | `arg1_col` |
|---|---:|---:|---:|---:|---:|
| `mov` | 3 | 8 | 15 | 4 | 11 |
| `section` / `syscall` | 7 | 8 | 19 | 4 | 15 |
| `cmpxchg16b` | 10 | 6 | 20 | 2 | 16 |
| `vpbroadcastmb2q` | 15 | 2 | 21 | 2 | 21 |
| `verylonginstruction` | 19 | 2 | 25 | 2 | 25 |

Directives participate, so `section` (7) alone lifts a small file to column 19.

### Example: long line sets the column

`M = 19`, `gap = 2`, `arg1_col = 25`:

```nasm
_start:
    syscall
    mov                  rax, 10
    verylonginstruction  100
```

The cost is visible here: the two short lines pay for the long one. That is the
accepted trade-off from decision #3. There is no silent exemption.

### Example: all mnemonics short

`M = 7` (`syscall`/`section`), `gap = 8`, `arg1_col = 19`:

```nasm
    section        .text
    global         _start

_start:
    mov            eax, 1
    syscall
```

The longest mnemonic gets exactly `gap` spaces; shorter ones get
`gap + (M - len(mnemonic))`.

### Operand columns (opt-in)

`align_operands = true` measures each operand field too, with the comma attached to
the operand it follows (`rax,`). Column `i+1` = column `i` + max width of field `i` +
gap. With `--align-gap 2` and `M = 4`:

```nasm
    add   rax,  rbx
    mov   rcx,  5
    imul  rdx,  rsi,  10
```

With the default `align_gap = 8` this mode is very wide; `--operand-gap N` overrides
the arg-to-arg gap independently when desired.

### Comments

```
comment_col = max(end of code over all lines) + comment_gap
```

`comment_gap` defaults to 4. Comments are placed at
`max(comment_col, code_len + min_gap)`, so a line that already reaches `comment_col`
still keeps at least `min_gap` before its `;`. `--comment-column N` forces an
absolute column. Comment-only lines and preprocessor lines remain at column 0.

```nasm
    mov            eax, 1    ; SYS_write
    syscall                  ; invoke
```

### Labels

- Label-only lines: `name:` at column 0, nothing else, right-trimmed.
- Inline `name: instruction` stays on the same line; the label is indented and the
  instruction follows, aligned normally.
- `equ` labels remain on a single line: `[label] equ [value]`.
- `.local`, `%%macro-local`, `..@`-style labels are recognized, never re-cased.
- A line with a label and no operands is not padded beyond the label.

### Directives

- Assembler directives (`section`, `segment`, `global`, `extern`, `default`, `bits`,
  `use16`/`use32`/`use64`, `org`, `align`, etc.) start at column 0 and are aligned
  among themselves.
- Data definition directives (`db`, `dw`, `dd`, `dq`, `dt`, `do`, `dy`, `dz`, `resb`,
  `resw`, `resd`, `resq`, `rest`, `reso`, `resy`, `resz`) follow instruction alignment
  rules (indented and aligned to the operand column like instructions).
- Preprocessor directives (`%...`) remain at column 0 and are excluded from alignment
  measurements.

### Blank lines and EOF

- Runs of blank lines collapse to one.
- Leading blank lines are removed.
- Exactly one trailing newline when the file is non-empty.
- No blank lines are inserted; the formatter does not invent structure.
- Line endings are detected per file and preserved (CRLF stays CRLF).

---

## Lexing rules

1. **Comments.** `;` starts a comment unless it is inside `'...'`, `"..."`, or
   `` `...` ``. A comment whose next character is alphanumeric gets a space after the
   semicolon (`;foo` → `; foo`); punctuation-led comments (`;---`, `;=`, `;;`) are left
   alone. An unterminated quote makes the line pass through untouched.
2. **Commas.** Operands split on commas at nesting depth 0 only. `[rbp-8]`, `(a,b)`,
   `{k1}`, and quoted strings do not split. `db "a,b", 0` is two operands.
3. **Brackets.** No space just inside `[` or `]`, and no spaces around `+ - *` in
   effective addresses: `[rbp-8]`, `[rax+rbx*4]`. Prefix keywords keep a space:
   `[rel msg]`, `byte [rbp-8]`. Switchable with `bracket_spacing = true`.
4. **Prefixes.** `lock`, `rep`, `repe`/`repz`, `repne`/`repnz`, `xacquire`,
   `xrelease`, `bnd`, `notrack`, `a16`/`a32`/`a64`, `o16`/`o32`/`o64`, and segment
   overrides are folded into the mnemonic field.
5. **Numbers.** Preserved byte-for-byte by default (`0xFF`, `0ffh`, `101b`, `1_000`).
   `--hex-case lower|upper` normalizes hex literals of the form `0x...`/`0X...`
   outside quotes and does not rename symbols containing hex digit sequences.
6. **Case.** Mnemonics and directive/preprocessor keywords are lowercased. Registers,
   symbols, labels, and everything inside quotes are never changed.
7. **Preprocessor.** `%...` lines stay at column 0 and are excluded from `M`.
   `%macro` bodies are formatted as normal code; `%endmacro`, `%endif`, `%endrep`
   return to column 0. `%if` bodies are not indented.
8. **Unparseable lines.** Passed through with trailing whitespace removed and excluded
   from column measurement. This includes malformed macro invocations the lexer cannot
   confidently split.

---

## CLI

```
clean_ass [OPTIONS] [FILE...]

  -i, --in-place              rewrite files (required for multiple paths)
  -c, --check                 exit 1 if any file would change; write nothing
      --stdin                 read stdin (also: no paths, or "-")
      --config <PATH>         explicit config file
      --indent <N>            default 4
      --align-gap <N>         default 8
      --min-gap <N>           default 2
      --soft-limit <N>        default 8
      --align-operands        align arg2, arg3, ... as columns
      --operand-gap <N>       gap between operands (default: adaptive gap)
      --comment-gap <N>       default 4
      --comment-column <N>    fixed comment column (default: auto)
      --bracket-spacing       put spaces inside brackets
      --hex-case <case>       preserve | lower | upper
      --max-width <N>         wrap long operand lists (default 0 = never)
  -h, --help / -V, --version
```

Exit codes: `0` success, `1` `--check` found differences, `2` usage or I/O error.
`--` ends option parsing. Multiple paths require `-i` or `--check`.

## Config file

`.clean_ass.toml` is discovered by walking up from each target file; `--config`
bypasses discovery. Flat `key = value` pairs in a TOML subset (strings, integers,
booleans, `#` comments, one level of `[table]`). CLI flags override the file.

```toml
indent          = 4
align_gap       = 8
min_gap         = 2
soft_limit      = 8
align_operands  = false
# operand_gap   = 2
comment_gap     = 4
# comment_column = 40
bracket_spacing = false
hex_case        = "preserve"
max_width       = 0
```

---

## Safety and idempotency

- **Columns come from token widths, never from existing whitespace.** Re-formatting
  formatted output yields identical bytes. This is tested on every fixture.
- **Nothing is evaluated.** `%define`, `equ`, `%assign`, and macros are treated as text.
- **Unknown constructs are preserved.** The formatter may be conservative, but it never
  guesses.
- **`--check` is byte-exact.** It is the intended CI gate.
- **No partial writes.** `-i` formats the whole file the same way stdout mode does
  before writing.

## Architecture

```
src/lib.rs      format_source(&[u8], &Config) -> Vec<u8>   (unit-testable core)
src/main.rs     file loop, stdin/stdout, -i, --check, exit codes
src/cli.rs      hand-rolled argument parsing
src/config.rs   defaults, TOML subset, upward discovery, flag merge
src/lexer.rs    Line { preproc, label, mnemonic, operands, comment, raw }
                quote-aware comment split, bracket-aware comma split
src/format.rs   measure pass -> column pass -> emit, blank/EOL policy, alignment
```

The formatter is two passes: parse every line into fields, compute per-field widths,
then emit. O(n) in file size, no allocation-heavy parsing, no regex.

## Testing

- `tests/fixtures/` pairs (`X.asm`, `X.expected.asm`) covering: `basic`,
  `adaptive_gap`, `operands_aligned`, `comments`, `strings_with_semicolons`,
  `brackets_and_times`, `directives`, `macros`, `crlf`, `garbage`.
- Idempotency test over every fixture: `format(format(input)) == format(input)`.
- Unit tests for the lexer: quotes, escapes, nested brackets, comma splitting,
  prefix folding, unterminated input.
- Exit-code tests for `--check` and error paths.
- Gate: `cargo test` and `cargo clippy -- -D warnings`.

## Implementation order

1. Config, CLI, I/O, `--check`, library skeleton.
2. Lexer + unit tests.
3. Measure/emit with adaptive gap; both operand modes.
4. Comment column, casing, blank-line/EOF policy.
5. Preprocessor and macro handling.
6. Config discovery.
7. Fixtures, idempotency, `--help` polish.
8. Optional: `--max-width` wrapping of long `db`/`dd` lists.

## Open questions

- Wrapping (`--max-width`): how continuation lines align with the operand columns,
  and whether wrapping should be enabled by default.
- Whether `%if`/`%else` bodies should be indented behind an option.
- Whether comments should be reflowed to `max_width` (currently never).
