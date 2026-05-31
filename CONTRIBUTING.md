# Contributing

Chirper changes should go through pull requests. Do not push directly to
`main` except for repository setup emergencies.

## Branch Flow

1. Start from an up-to-date `main`.
2. Create a focused branch:

   ```sh
   git checkout main
   git pull --ff-only origin main
   git checkout -b codex/short-description
   ```

3. Keep the change scoped. Avoid unrelated formatting or refactors.
4. Run the relevant checks locally.
5. Push the branch and open a draft PR.
6. Move the PR out of draft only after tests pass and the diff is ready for
   review.

## Required Local Checks

For Rust changes:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

For GNOME/GJS app or extension changes:

```sh
find extensions apps -name '*.js' -exec node --check {} \;
```

For shell script changes:

```sh
bash -n scripts/*.sh
```

## Review Expectations

- PRs should explain what changed, why, and how it was tested.
- Bug fixes should describe the user-visible failure or root cause.
- UI changes should include screenshots or a short manual test note when
  possible.
- Do not merge with failing CI unless the failure is unrelated and documented.

## Recommended Repository Ruleset

Configure this in GitHub under `Settings -> Rules -> Rulesets` for `main`:

- Require a pull request before merging.
- Require at least one approval.
- Require review from Code Owners.
- Require conversation resolution before merging.
- Require status checks to pass before merging.
- Require branches to be up to date before merging.
- Block force pushes.
- Block branch deletion.

Use the `CI` workflow as a required status check once GitHub has seen the first
workflow run.
