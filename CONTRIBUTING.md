# Contributing

Thanks for looking in. This project is developed in the open and small, focused
contributions are the easiest to review.

## Before you start

- **Bugs** — open an issue with the version/tag and the exact artifact you ran
  (`mbscada-rs` version from `--version`, or the release asset name).
- **Features** — open a Discussion first if the change is larger than a one-liner; it saves
  you from writing code that has to be thrown away.
- **Security** — please **do not** open a public issue; see `SECURITY.md`.

## Pull requests

1. Fork, branch from the default branch, keep the change focused.
2. Match the existing style; run the project's own checks before pushing
   (`cargo test`, `pytest`, `make`, whatever the CI runs — see `.github/workflows`).
3. Update `README.md` / docs if behaviour or flags changed.
4. Write a commit subject in the imperative mood (`fix: ...`, `feat: ...`, `docs: ...`).
5. In the PR description say **what problem** it solves and **how you verified** it. A CI
   link or a pasted command output beats "it works".

## Good first issues

Issues labelled [`good first issue`](https://github.com/kostyk348/mbscada-rs/labels/good%20first%20issue)
are scoped to be done in one sitting, with the acceptance criteria written in the issue body.
If one is unclear, ask in the issue — that is a documentation bug and we will fix it.

## Code of conduct

Be technical, be kind. Disagreement about code is welcome; dismissal of people is not.
