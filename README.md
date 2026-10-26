# clean_ass

A formatter for NASM/YASM assembly files (Intel syntax). Deterministic, opinionated, zero-dependency.

## Disclaimer

**This project is provided AS IS.** The author is not responsible for any bugs, mistakes, or issues in this code. This project is completely **vibe-coded** with **0 handwritten code or manual verifications**. It was developed entirely using AI assistance.

## Development

- **AI Agent:** OpenCode (coding agent harness)
- **Model:** Big Pickle (provider: opencode, model ID: big-pickle)
- **Development Method:** Vibe-coded via OpenCode with no handwritten code or manual verification performed
- **Language:** Rust
- **Dependencies:** None (hand-rolled CLI and TOML subset)

## What it does

- Formats NASM/YASM Intel-syntax assembly files
- Aligns operands to columns for readability
- Preserves semantics - never rewrites instructions, macros, or string contents
- Idempotent formatting
- Handles preprocessor directives, macros, labels, comments

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/clean_ass`.

## Usage

```bash
# Format a file and print to stdout
clean_ass file.asm

# Format in place
clean_ass -i file.asm

# Check if files would be formatted (CI)
clean_ass --check file.asm

# Read from stdin
clean_ass --stdin < file.asm
cat file.asm | clean_ass
```

## Key Behaviors

### Directives
- Assembler directives (`section`, `global`, `extern`, `default`, `bits`, `org`, `align`, etc.) are flush-left with a 2-space gap to operands
- Data definition directives (`db`, `dw`, `dd`, `dq`, `dt`, `resb`, `resw`, `resd`, `resq`, etc.) follow instruction alignment rules (indented)
- Preprocessor directives (`%...`) remain at column 0

### Labels
- Label-only lines stay at column 0
- Inline labels (`label: instruction`) stay on the same line, indented with instruction aligned to arg1 column
- `equ` labels stay on the same line

### Formatting
- Mnemonics and directives lowercased, symbols/registers/strings preserved
- One operand column for the entire file (longest mnemonic sets the column)
- Adaptive gaps - generous for short mnemonics, tighter for long ones
- Comments aligned to the longest code line
- Semicolons in quotes are not treated as comments
- CRLF line endings preserved

## Options

```
  -i, --in-place              rewrite files (required for multiple paths)
  -c, --check                 exit 1 if any file would change
      --stdin                 read stdin
      --config <PATH>         explicit config file
      --indent <N>            default 4
      --align-gap <N>         default 8
      --min-gap <N>           default 2
      --soft-limit <N>        default 8
      --align-operands        align arg2, arg3, ... as columns
      --operand-gap <N>       gap between operands
      --comment-gap <N>       default 4
      --comment-column <N>    fixed comment column
      --bracket-spacing       put spaces inside brackets
      --hex-case <case>       preserve | lower | upper
      --max-width <N>         wrap long operand lists (default 0 = never)
  -h, --help
  -V, --version
```

## Configuration

Create `.clean_ass.toml` in your project (discovered by walking up from target files):

```toml
indent          = 4
align_gap       = 8
min_gap         = 2
soft_limit      = 8
align_operands  = false
comment_gap     = 4
bracket_spacing = false
hex_case        = "preserve"
max_width       = 0
```

## Development

```bash
cargo test        # Run tests
cargo clippy -- -D warnings
cargo fmt --check
```

## License

See repository for license information.
