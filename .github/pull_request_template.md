## Summary

- Describe the change and the problem it solves.
- Link any related issue: `Closes #...` or `Refs #...`

## What Changed

- List the main code, docs, or test changes.
- Call out any behavior changes that reviewers should verify.

## Validation

- List the commands you ran locally.
- Include the smallest relevant test coverage first.

Example:

```bash
cargo test --lib
cargo test --all-features
mdbook build
```

## Review Notes

- Highlight feature flags, backend-specific behavior, migrations, or follow-up work.
- Note any intentionally deferred work or known limitations.

## Checklist

- [ ] The change is scoped and ready for review.
- [ ] Relevant tests pass locally.
- [ ] Tests were added or updated where practical.
- [ ] Documentation was updated if user-facing behavior changed.
- [ ] Feature-gated behavior stays aligned across runtime, macros, tests, and docs.
- [ ] Generated output under `site/` was not edited by hand.
- [ ] This PR does not disclose a private security issue. If it does, I will follow `SECURITY.md` instead of opening a public PR.
