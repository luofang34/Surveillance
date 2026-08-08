# Continuous integration

## Gates

Two scripts define every check, and both local development and hosted CI run
exactly them, so the two cannot drift:

```sh
scripts/ci.sh            # fmt --check, clippy -D warnings, tests, rustdoc, release build
scripts/check-no-std.sh  # thumbv6m, thumbv8m.main, riscv32imac, wasm32-unknown-unknown
```

The workflow in `.github/workflows/ci.yml` invokes those scripts and adds
only the things a hosted runner needs: checking out the dependency,
installing targets, and caching.

## The private dependency

`surveillance-core` depends on `aero-link` through a path dependency
(`../aero-link`), and that repository is private. A hosted runner therefore
cannot build anything until it has read access to a second repository, which
is the whole reason this workflow needs a credential at all.

### Credential

CI authenticates with a **read-only deploy key** rather than a personal
access token. A deploy key is scoped to one repository and one permission,
so a leak from CI exposes read access to `aero-link` and nothing else — no
other repository, and no ability to write anywhere. A personal access token,
even a fine-grained one, is bound to a user account and is a broader blast
radius for the same job.

The key pair is already installed:

- the public half is a read-only deploy key on `luofang34/aero-link`, titled
  `Surveillance CI (read-only)`
- the private half is the `AERO_LINK_DEPLOY_KEY` Actions secret on
  `luofang34/Surveillance`

Both halves are replaceable without touching the workflow:

```sh
ssh-keygen -t ed25519 -f aero-link-ci -N "" -C "surveillance-ci-readonly"
gh repo deploy-key add aero-link-ci.pub -R luofang34/aero-link -t "Surveillance CI (read-only)"
gh secret set AERO_LINK_DEPLOY_KEY -R luofang34/Surveillance < aero-link-ci
rm aero-link-ci aero-link-ci.pub
```

Revoke an old key with `gh repo deploy-key delete <id> -R luofang34/aero-link`.

A missing secret fails the job on its own step with an explicit message
rather than surfacing as a confusing checkout error, so a revoked or expired
credential is never mistaken for a code failure.

To switch to a fine-grained personal access token instead — for example if
CI ever needs to write to `aero-link` — grant it `Contents: read` on that
repository only, store it as `AERO_LINK_TOKEN`, and replace the `ssh-key:`
input on the aero-link checkout step with `token:`. Nothing else changes.
Note that a fine-grained token cannot be created through the GitHub API; it
has to be generated in account settings.

### Revision

`ci/aero-link.ref` names the revision of `aero-link` to build against. It
holds `main` by default, which means Surveillance CI tracks the dependency
and reports a break as soon as one lands upstream. Writing a commit SHA into
that file pins the build instead, which is worth doing while chasing an
upstream regression. Either way the resolved commit is written to the job
summary, so a failure can always be attributed to a specific dependency
state.

Because the dependency is a path dependency, `--locked` builds fail when
`aero-link` changes its manifest without a matching `Cargo.lock` update here.
That failure is intended: the fix is a lockfile commit, not a relaxed gate.

## Checkout layout

The path dependency resolves to `../aero-link`, so the workflow checks both
repositories out as siblings:

```text
$GITHUB_WORKSPACE/
  Surveillance/
  aero-link/
```

Every cargo command runs from `Surveillance/`.
