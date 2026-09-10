# Contributing

Everyone's welcome!
 - New issues (feature requests, bug reports, etc)
 - Pull requests (doc/code refactors or improvements, new features, etc)

Both are much appreciated!

Please try to push high quality code when submitting your PR. If it fails testing it will be denied. 
If you're resolving an issue that was opened, please link to the issue in your PR. 

## Build & test

```bash
cargo build --release
cargo test
cargo clippy --release --all-targets -- -D warnings
```

All three must pass before a PR is reviewed.

Use Conventional Commits. PR Description should consist of the problem, summary of the fix, and tests performed.


This project's whole point is speed. Anything that changes the behavior needs a detailed explanation as to why you're introducing the change, how it will improve performance, benchmarks you've ran, and all testing.
Before adding a dependency, allocation, or check to a hot path, ask whether it's needed there or could move to setup/cold paths instead.

All contributors welcome! 
