# Repository Guidelines

## Project Structure & Module Organization

This directory currently contains design documents, not an implemented monorepo:

- `bld1.md`: proposed architecture, interfaces, database schema, and Rust host example.
- `bld2.md`: governance and training-data exporter examples.
- `bld2a.md`: privacy airlock and replay-oracle example.

Paths such as `host-kernel/`, `contracts/`, and `swarm-factory/` describe a proposed layout in `bld1.md`; do not assume those directories or their capabilities exist. Keep new material clearly labeled as design, example, or verified implementation.

## Development & Verification

There is currently no build manifest, installed test suite, or repository-wide run command. Review the documents with `rg -n 'search term' bld*.md` and inspect changes with `git diff --check` when working in a Git checkout. Code inside fenced blocks is illustrative until extracted, built, and tested independently. Document the environment and commands when verifying an example.

## Style & Naming

Use descriptive Markdown headings, short paragraphs, fenced code blocks with language identifiers, and backticks for paths and identifiers. Preserve the existing interface names and distinguish proposed paths from actual files. For code examples, follow the surrounding language conventions: four-space indentation for Python and idiomatic Rust formatting. Avoid introducing a formatter or lint requirement without configuring it.

## Testing & Evidence

When adding executable code, include focused tests for its behavior and document how to run them. The examples include smoke-test snippets, but their presence does not establish that they pass. Treat mock replay results, privacy claims, and governance outcomes as unverified until supported by actual test evidence; never equate a simulation with production validation.

## Commits & Pull Requests

No Git history is available in this directory, so no existing commit convention can be inferred. Use concise, descriptive commit subjects. In pull requests, explain the change, identify affected documents or contracts, link relevant issues when available, and report verification performed and known limitations.

## Security & Configuration

Keep secrets and sensitive data out of examples and commits. The proposed privacy airlock and sandbox are design claims; verify their boundaries before using them with private data or external services.
