#!/usr/bin/env sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
parser_root="$repo_dir/vendor/difftastic/vendored_parsers"

for source_dir in "$parser_root"/*-src; do
    resolved_dir=$(CDPATH= cd -- "$source_dir" && pwd -P)
    case "$resolved_dir/" in
        "$parser_root/"*) ;;
        *)
            printf '%s\n' "error: vendored parser resolves outside the repository: $source_dir -> $resolved_dir" >&2
            exit 1
            ;;
    esac

    for required_file in parser.c scanner.c tree_sitter/parser.h; do
        if [ ! -f "$resolved_dir/$required_file" ]; then
            printf '%s\n' "error: vendored parser source is missing: $resolved_dir/$required_file" >&2
            exit 1
        fi
    done
done

printf '%s\n' "Vendored parser sources are self-contained."
