# ADR-0018: Preflight is the arbiter, not a hosted pipeline

## Context

This repository ships three workflow definitions under `.github/workflows/`: a
`check` job that runs `scripts/preflight.sh` verbatim plus a two-target
bit-exactness matrix, a nightly job that runs the decoder campaign at its
declared budget and re-derives all sixty rate–distortion points, and a job that
builds and publishes the projection room.

None of them has ever executed. The repository's only remote is a GitLab
project, and GitHub Actions do not run there. The workflows are correct — they
are SHA-pinned, they read the Rust version from `rust-toolchain.toml` rather
than duplicating it, and the nightly job deliberately names no iteration count
so the declared budget stays the only copy — but correctness is not execution.

That was invisible for as long as it was, because several documents described
what the pipeline does in the present tense, which reads as a report. The
README said conformance hashes were "reproduced on both native targets in
continuous integration". `docs/LIMITATIONS.md` said both supported platforms
were "reproduced in continuous integration and required to be byte-identical".
`docs/writeups/05` opened by stating that the decoders "have run twenty million
iterations a night across four campaigns". Each of those describes a workflow
file. None of them described anything that had happened.

The distinction matters more here than it would in most repositories, because
the whole claim of this project is that nothing on its pages has to be taken on
trust. A claim resting on a pipeline the reader cannot see run is exactly the
kind of claim the repository exists to not make.

## Decision

`scripts/preflight.sh` is the arbiter. Every claim this repository makes names
a check that a reader can run on their own machine with one command, and no
document states a result that only a hosted pipeline could produce.

The GitHub Actions definitions stay as they are. They are the pipeline this
repository would run if it were hosted where they run, they are the form every
sibling repository uses, and deleting a correct definition to reflect a hosting
choice would lose work and prove nothing. Where a workflow does something
preflight cannot — the second native target, the full campaign budget, all
sixty receipt points — the documents say that it is configured to, and say
plainly that it has not.

Whether the repository moves to a host that runs these workflows is an owner
decision about hosting, not an engineering decision about the codec, and this
record does not make it.

## Consequences

`docs/LIMITATIONS.md` names the unexecuted pipeline as a limitation in the
section that promises to name every one, which is where a reader looking for
what this project has not done will look.

Platform coverage is now stated as what has been reproduced rather than as what
the matrix would reproduce. Both native targets remain supported and the
conformance suite is reproduced on whichever one runs preflight; only one of
them has ever done so.

`scripts/check-doc-coherence.mjs` holds the README's `## Verify` block to
preflight step for step, so the list a reader is told to run cannot fall behind
the gates that actually run. It had fallen three behind.

The claims table keeps naming the nightly job where the nightly job is what
would re-derive a figure, because describing a workflow's configuration is
accurate. What it no longer does is offer that configuration as evidence.

Earlier records are not edited. ADR-0009 states that continuous integration
reproduces every origin's stream bytes on both native targets, and ADR-0017
that the nightly job drives twenty million mutated streams through both
decoders — the second understating its own subject, since the declared budget
is twenty million per target and there are four. Both were accurate about what
the workflows say and neither was ever accurate about what had run. They stay
as written, because these records are append-only and the reasoning at the time
is the part worth keeping; this record is where a reader learns that the
pipeline those two describe has never had a runner.

## Alternatives considered

- **Port the pipeline to `.gitlab-ci.yml` and retire the workflows.** Rejected.
  The macOS runner needed for the second half of the bit-exactness matrix is
  not on the free tier, so the matrix would still not run and the honest
  statement about platforms would be unchanged. It would also put this
  repository's pipeline in a form none of its siblings use, for a benefit that
  is entirely hypothetical until someone enables a runner.
- **Keep both a GitLab pipeline and the workflows.** Rejected for the same
  reason plus a worse one: two definitions of one gate list is the defect this
  repository has spent most of its audits closing, and neither copy would be
  the one anybody ran.
- **Soften the prose to the future tense — "will be reproduced in continuous
  integration".** Rejected. A promise about a pipeline nobody has scheduled is
  still a claim a reader cannot check, and it reads as though the schedule
  exists.
- **Delete the workflows.** Rejected. They encode a real and reviewed decision
  about what the full gate list is and where it should run, and the day the
  repository moves they are what it needs.

## Supersedes / superseded by

Nothing. ADR-0001 records the original decision to gate the dependency graph in
continuous integration; this record does not reverse it, it states what that
gating has and has not done.
