# A directory is an argument, not process state

## Why

`cargo test --workspace` failed on
`manifest::override_tests::a_written_rule_set_replaces_the_shipped_one`, and
passed when the test was run alone. Three consecutive runs of the crate's own
suite: ok, **failed**, ok.

The two override tests each pointed `OMT_AGENT_RULES` at a temporary directory
and removed it afterwards, through a helper carrying this comment:

> SAFETY: these tests are single-threaded within this module and the variable
> exists to be set.

That is not true. libtest runs the tests in one binary on several threads, so
the second test removed the variable while the first was still reading it. The
failure looks like the loader ignoring a written rule set, which is a real bug
somebody would then go hunting for.

An environment variable is process state. Two callers wanting different values
at once cannot both have one, and the loser gets a wrong answer rather than an
error.

## What changes

- `manifest::load_from(dir, agent)` and `AgentAdapter::rules_in_use_from(dir)`:
  a caller that knows where to look says so.
- `manifest::load` and `rules_in_use` keep their meaning and consult the
  environment exactly once, at the edge, which is the only place process state
  belongs.
- The two tests take a path instead of setting a variable, and the comment
  that was not true is gone with the code it described.

## Non-goals

- No change to where rules are looked for by default, nor to what a rule file
  means.
- No sweep of every other environment variable read in a test; this fixes the
  one with a reproduced failure.

## Impact

- `omt-agent-adapters`: one loader function and one trait method, both additive.
- Depends on nothing.
