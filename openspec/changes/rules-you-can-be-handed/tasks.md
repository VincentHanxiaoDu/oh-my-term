## 1. Capture

- [x] 1.1 `agent_rules.capture`: the visible screen of a pane
- [x] 1.2 Each named region marked out, so a rule can be written against what
      it actually contains
- [x] 1.3 Says which agent omt believes is there, or that it identified none
- [x] 1.4 Written as a file somebody can edit into rules

## 2. Import

- [x] 2.1 `agent_rules.inspect`: what a set would change, without changing it
- [x] 2.2 `agent_rules.import`: install a checked set
- [x] 2.3 A set for another agent is refused, naming both
- [x] 2.4 A rule with no test is refused, naming that rule
- [x] 2.5 A set needing a newer engine is refused whole
- [x] 2.6 Nothing is fetched or applied at startup, ever

## 3. Trace

- [x] 3.1 `agent_rules.explain`: which rule won, which region, what it concluded
- [x] 3.2 Nothing matching says so rather than naming a rule

## 4. Reachable

- [x] 4.1 The three are in the catalog and the parity gate
- [x] 4.2 `docs/reference/agent-rules.md` covers capture and import

## 5. Close-out

- [x] 5.1 clippy, workspace tests, layering, codegen, test-isolation
- [x] 5.2 `npm run check`
