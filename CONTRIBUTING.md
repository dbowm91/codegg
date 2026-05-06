# Contributing to codegg

Thank you for your interest in contributing to codegg!

## Getting Started

### Prerequisites

- Rust 1.81 or later
- Cargo

### Development Setup

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_HANDLE/codegg`
3. Create a branch: `git checkout -b feature/your-feature-name`
4. Make your changes
5. Run tests: `cargo test`
6. Format code: `cargo fmt --check`
7. Lint: `cargo clippy -- -D warnings`
8. Commit your changes
9. Push to your fork and submit a pull request

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and address all warnings (errors in CI)
- Follow existing code conventions and patterns
- Keep functions small and focused
- Document public APIs with doc comments

## Testing

All new functionality should include tests:

```bash
cargo test
```

For integration tests, see the `tests/` directory.

## Commit Messages

- Use clear, descriptive commit messages
- Start with a verb (Add, Fix, Update, Remove, etc.)
- Keep the first line concise (under 72 characters)
- Add body text for complex changes

## Pull Requests

- Keep PRs focused and reasonably sized
- Reference issues where applicable
- Ensure CI passes before requesting review
- Update CHANGELOG.md for user-facing changes

## Reporting Issues

When reporting bugs, include:

- Rust version (`rustc --version`)
- Operating system
- Steps to reproduce
- Expected vs actual behavior
- Any relevant logs or error messages

## Questions?

Feel free to open a discussion or ask questions in GitHub Discussions.
