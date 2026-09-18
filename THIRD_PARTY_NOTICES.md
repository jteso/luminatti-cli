# Third-party notices

## difftastic

This repository retains a vendored copy of the structural diff engine from
[Wilfred/difftastic](https://github.com/Wilfred/difftastic), pinned from the
upstream commit `fd90d3b9a65530848f97154cb8b8ea245c1aa4bc` on 2026-09-18.
Difftastic is MIT licensed; its full
license is retained at `vendor/difftastic/LICENSE`.

Difftastic includes Tree-sitter parsers. Their individual licenses remain in
`vendor/difftastic/vendored_parsers/*/LICENSE` where applicable. Luminatti
links the vendored engine in-process and renders its structural line alignment,
token changes, and fallback results in the TUI.
